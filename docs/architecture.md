# webhooks.live architecture

Status: draft 2, 2026-11-21 (parity design §16–§36). Requirements: [product-spec.md](product-spec.md). HTTP contract: [api.md](api.md).
Every section states the decision, why, and the main alternative that was rejected.

## 1. Shape of the system

One Rust binary `webhooks-live` serves three things on one port: the public capture path, the management API with
its live stream, and the embedded React UI. Postgres is the only stateful dependency; it holds data, the
forwarding queue, sessions, and carries cross-replica events through `LISTEN/NOTIFY`. Replicas are identical
and stateless apart from in-process rate-limit counters and live-stream fan-out.

```
sender ──► Envoy ──► replica A ──────► Postgres ◄── replica B ◄────── Envoy ◄── viewer (SSE)
                         │  INSERT + pg_notify  │  LISTEN whl_events   │
                         └──── forward worker (SKIP LOCKED) ──► target URL
```

## 2. Storage

**Decision: Postgres 16+ only, logged tables, bodies as `bytea` in a separate table with lz4 TOAST compression.**

Postgres meets every constraint with one dependency: durable across restarts (O5), shared by all replicas
without sticky sessions (O4), transactional counting for the 500-capture limit and ring mode, a queue for
forwarding (`FOR UPDATE SKIP LOCKED`) and a pub/sub channel (`LISTEN/NOTIFY`). A 10 MiB `bytea` is well inside
TOAST limits; keeping bodies in their own table means list queries never read or detoast them. The capture
write is one short transaction and never waits for viewers: fan-out happens after commit on the listener side.

Rejected: **Redis** (the old defect: no relational limits, memory-bound bodies, a second stateful service);
**SQLite** (single writer, cannot be shared by several replicas); **Postgres + S3 for bodies** (a second store
and non-transactional deletes for no gain at a 10 MiB ceiling); **UNLOGGED tables** (truncated on crash and
absent after failover, which breaks O5).

### 2.1 Schema

```sql
CREATE TABLE users (
  id            uuid PRIMARY KEY,
  issuer        text NOT NULL,
  subject       text NOT NULL,
  email         text NOT NULL,
  display_name  text,
  created_at    timestamptz NOT NULL DEFAULT now(),
  last_login_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (issuer, subject)
);
CREATE INDEX users_email ON users (lower(email));

CREATE TABLE sessions (
  token_hash   bytea PRIMARY KEY,
  user_id      uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  created_at   timestamptz NOT NULL DEFAULT now(),
  last_seen_at timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL
);
CREATE INDEX sessions_expires ON sessions (expires_at);

CREATE TABLE login_states (
  state         text PRIMARY KEY,
  pkce_verifier text NOT NULL,
  nonce         text NOT NULL,
  return_to     text NOT NULL,
  expires_at    timestamptz NOT NULL
);

CREATE TABLE endpoints (
  id                    text PRIMARY KEY CHECK (id ~ '^[0-9a-hjkmnp-tv-z]{24}$'),
  owner_id              uuid REFERENCES users,
  title                 text,
  response_status       smallint NOT NULL DEFAULT 200 CHECK (response_status BETWEEN 100 AND 599),
  response_content_type text NOT NULL DEFAULT 'text/plain',
  response_headers      jsonb NOT NULL DEFAULT '[]',
  response_body         bytea NOT NULL DEFAULT '',
  response_delay_ms     integer NOT NULL DEFAULT 0 CHECK (response_delay_ms BETWEEN 0 AND 10000),
  cors                  boolean NOT NULL DEFAULT false,
  overflow              text NOT NULL DEFAULT 'refuse' CHECK (overflow IN ('refuse', 'ring')),
  capture_count         integer NOT NULL DEFAULT 0,
  bytes_used            bigint NOT NULL DEFAULT 0,
  last_seq              bigint NOT NULL DEFAULT 0,
  created_at            timestamptz NOT NULL DEFAULT now(),
  last_active_at        timestamptz NOT NULL DEFAULT now(),
  deleted_at            timestamptz
) WITH (fillfactor = 80);
CREATE INDEX endpoints_owner ON endpoints (owner_id) WHERE owner_id IS NOT NULL;
CREATE INDEX endpoints_deleted ON endpoints (deleted_at) WHERE deleted_at IS NOT NULL;

CREATE TABLE endpoint_members (
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  user_id     uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  role        text NOT NULL CHECK (role IN ('viewer', 'editor')),
  created_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (endpoint_id, user_id)
);
CREATE INDEX endpoint_members_user ON endpoint_members (user_id);

CREATE TABLE captures (
  endpoint_id     text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  seq             bigint NOT NULL,
  id              uuid NOT NULL UNIQUE,
  received_at     timestamptz NOT NULL,
  method          text NOT NULL,
  url             text NOT NULL,
  path            text NOT NULL,
  host            text NOT NULL,
  client_ip       inet NOT NULL,
  headers         jsonb NOT NULL,
  query           jsonb NOT NULL,
  form            jsonb,
  content_type    text,
  body_size       integer NOT NULL,
  stored_bytes    integer NOT NULL,
  status_override smallint,
  response_status smallint NOT NULL,
  PRIMARY KEY (endpoint_id, seq)
);

CREATE TABLE capture_bodies (
  capture_id  uuid PRIMARY KEY REFERENCES captures (id) ON DELETE CASCADE,
  data        bytea COMPRESSION lz4 NOT NULL,
  search_text text COMPRESSION lz4 NOT NULL
);

CREATE TABLE forward_rules (
  id               uuid PRIMARY KEY,
  endpoint_id      text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  position         smallint NOT NULL,
  enabled          boolean NOT NULL DEFAULT true,
  target_url       text NOT NULL,
  method           text,
  content_type     text,
  header_allowlist text[] NOT NULL DEFAULT '{}',
  append_path      boolean NOT NULL DEFAULT false,
  UNIQUE (endpoint_id, position)
);

CREATE TABLE forward_jobs (
  id               uuid PRIMARY KEY,
  capture_id       uuid NOT NULL REFERENCES captures (id) ON DELETE CASCADE,
  rule_id          uuid REFERENCES forward_rules ON DELETE SET NULL,
  kind             text NOT NULL CHECK (kind IN ('rule', 'manual')),
  target_url       text NOT NULL,
  method           text NOT NULL,
  content_type     text,
  header_allowlist text[] NOT NULL,
  state            text NOT NULL CHECK (state IN ('pending', 'running', 'succeeded', 'failed')),
  attempts         smallint NOT NULL DEFAULT 0,
  next_attempt_at  timestamptz NOT NULL DEFAULT now(),
  locked_until     timestamptz,
  created_at       timestamptz NOT NULL DEFAULT now(),
  finished_at      timestamptz
);
CREATE INDEX forward_jobs_due ON forward_jobs (next_attempt_at) WHERE state IN ('pending', 'running');
CREATE INDEX forward_jobs_capture ON forward_jobs (capture_id);

CREATE TABLE forward_attempts (
  job_id       uuid NOT NULL REFERENCES forward_jobs ON DELETE CASCADE,
  attempt      smallint NOT NULL,
  started_at   timestamptz NOT NULL,
  duration_ms  integer NOT NULL,
  status       smallint,
  error        text,
  final_url    text,
  response_content_type text,
  response_head bytea,
  PRIMARY KEY (job_id, attempt)
);
```

| Rule | Detail |
|---|---|
| Capture insert | One transaction: `UPDATE endpoints SET capture_count+1, bytes_used+n, last_seq+1, last_active_at=now() WHERE id=$1 AND deleted_at IS NULL AND last_active_at > now()-retention AND (capture_count < max OR overflow='ring') RETURNING last_seq, response_*`; ring mode then deletes `min(seq)` and corrects counters; `INSERT captures`, `INSERT capture_bodies`, `INSERT forward_jobs` per enabled rule, `pg_notify`. Zero rows from the `UPDATE` triggers one `SELECT` to tell gone (410) from full (410 with `X-Webhooks-Live-Error: endpoint_full`). |
| Ordering | `seq` is assigned under the endpoint row lock, so per-endpoint commit order equals `seq` order across replicas. It is the cursor, the SSE event id, and the `after_seq` filter the UI uses to catch up after a reconnect. `id` is a UUIDv7 (`X-Request-Id`, URLs). |
| `stored_bytes` | `body_size` plus serialized lengths of headers, query, form, `search_text`. Sum feeds `bytes_used` and the global cap. |
| `search_text` | Lowercased lossy UTF-8 of path, query, header lines and the first 256 KiB of the body. Search is `strpos(search_text, lower($q)) > 0` inside one endpoint (at most 500 rows); no trigram index, to keep the write path cheap. |
| No index on `last_active_at` | Keeps endpoint updates HOT; the eviction job sorts the endpoints table, which stays small. |
| Endpoint delete | Sets `deleted_at` and answers at once; the purge job deletes rows in batches. Clear-all and single capture delete are synchronous. |
| Counter drift | The maintenance job recomputes `capture_count`/`bytes_used` from `captures` hourly. |

## 3. Live updates

**Decision: Server-Sent Events, fanned out across replicas with Postgres `LISTEN/NOTIFY`, resumable by
`Last-Event-ID` = capture `seq`.**

Traffic is one-way, `EventSource` reconnects by itself and replays `Last-Event-ID`, cookies flow on the same
origin, and plain HTTP passes Envoy and HTTP/2 multiplexing without upgrade handling. Postgres already holds
the data, so `NOTIFY` inside the capture transaction is delivered exactly on commit and adds no new service;
the payload stays under the 8000-byte limit because the stream carries summaries only.

Rejected: **WebSocket** (bidirectional channel not needed, no built-in resume); **Redis pub/sub** (second
stateful service for a job Postgres does). Known ceiling: `NOTIFY` serialises committing transactions on a
global lock; that is far above the per-endpoint rate limits. If it is ever hit, move `pg_notify` out of the
transaction into a post-commit notifier task; the event contract does not change.

| Piece | Behaviour |
|---|---|
| Channel | `whl_events`. Capture: `{"t":"capture","e":<endpoint id>,"seq":n,"s":<summary>}`, path in the summary truncated to 1 KiB. Other events: `{"t":"capture_deleted"\|"captures_cleared"\|"endpoint_updated"\|"endpoint_deleted"\|"forward_result","e":<endpoint id>,"d":<data>}`, where `d` is the event's data object exactly as in api.md §6 and is omitted when the event has none. |
| Listener | One `sqlx::postgres::PgListener` per replica, holding one pool connection for its lifetime. After every successful `LISTEN`, including the first, it broadcasts one `resync` to every local subscriber, since events may have been missed while it was not listening. |
| Hub | `HashMap<EndpointId, broadcast::Sender<Arc<Event>>>`, capacity 256, entry removed when the last receiver drops. Events for endpoints with no local viewer are discarded after decoding. |
| Slow viewer | A lagging receiver gets `RecvError::Lagged`, the stream sends `resync` and continues. Viewers never back-pressure the listener or the capture path. |
| Connect | `ready` is always the first event of every connection, then any replay, then live events. The resume point comes from `?last_event_id` and the `Last-Event-ID` header; when both are present the larger wins, and a malformed value is ignored. The stream subscribes to the hub first, then reads `seq >` resume point from the database (at most 100 rows, ascending), then forwards live events, skipping `seq` already replayed. More than 100 missing sends `resync`. Without a resume point there is no replay. |
| Liveness | Comment ping every 15 s, `retry: 2000`, server closes each stream after 30 minutes so clients rebalance across replicas. Connecting and every reconnect extend endpoint lifetime. |
| Shutdown | Streams receive `bye` and close; clients reconnect to another replica and resume by `Last-Event-ID`. A connection that arrives during drain gets 200 and an immediate `bye`. |

## 4. Workspace and crates

**Decision: Cargo workspace `crates/*`, edition 2024, resolver 3, toolchain pinned in `rust-toolchain.toml`
(1.98.0), same layout and profiles as `astrum-repository`.**

```
webhooks-live/   GlobalArtInc/webhooks-live
  Cargo.toml  rust-toolchain.toml  Containerfile
  crates/whl-{core,store,capture,live,forward,auth,api,web,telemetry,server,testkit}
  crates/whl-store/migrations/
  web/          React + TypeScript + Vite, pnpm
  e2e/          Playwright
  tools/        test-db.sh, loadtest/
  docs/
```

| Crate | Owns |
|---|---|
| `whl-core` | Domain types and pure rules: `EndpointId`, `CaptureId`, `ResponseConfig` validation, header and content-type validation, status override parsing, config structs. No I/O. |
| `whl-store` | `sqlx` pool, all SQL, migrations (embedded with `sqlmig`), `webhooks-live migrate` implementation. |
| `whl-capture` | The capture handler: id routing, body limit, client IP, parsing, store call, response shaping, CORS, delay. |
| `whl-live` | `PgListener`, hub, SSE encoding, replay. |
| `whl-forward` | SSRF-guarded HTTP client, job claim loop, retries, result storage. |
| `whl-auth` | OIDC flow, sessions, origin check, access decisions (`Role` per endpoint). |
| `whl-api` | Management REST handlers and DTOs, error mapping, rate-limit headers. |
| `whl-web` | Embedded SPA assets and their serving rules. |
| `whl-telemetry` | Tracing subscriber (JSON), metrics registry and series names. |
| `whl-server` | Binary `webhooks-live`: config load, router assembly, admin listener, background jobs, graceful shutdown. |
| `whl-testkit` | Per-test database on a podman Postgres, app spawner, HTTP and SSE test clients. |

| Concern | Choice | Why; rejected |
|---|---|---|
| Web framework | `axum` 0.8 on `hyper-util` auto (HTTP/1.1 + h2c) | House standard, tower middleware; TLS ends at Envoy. |
| DB driver | `sqlx` 0.8, runtime-checked queries | Needs `PgListener` and `bytea` streaming; schema is ten tables. Rejected `sea-orm` (house default in Astrum): an entity layer with no value here and no LISTEN. |
| Migrations | `sqlmig` (house), pairs `<ts>-<slug>.up/down.sql`, embedded; `webhooks-live migrate up|status` | One image and one tag; schema always matches the binary. Rejected the separate `Dockerfile.migrations` image: two artefacts to keep in lockstep. |
| Config | `figment`: defaults, optional TOML (`WEBHOOKS_LIVE_CONFIG`), env `WEBHOOKS_LIVE_*` last | Used across GlobalArt Rust services. Durations use the in-house parser in `whl-core`: whole numbers with units `ms`, `s`, `m`, `h`, `d`, `w`, compounds such as `1h30m`, lowercase, no spaces. |
| Tracing | `tracing`, `tracing-subscriber` JSON, `tower-http` `TraceLayer` | House standard. |
| Metrics | `metrics` + `metrics-exporter-prometheus` | Macro facade, one recorder. |
| HTTP client | `reqwest` (rustls, custom resolver, redirects off) | SSRF guard needs control of resolution and every hop. |
| OIDC | `openidconnect` | Discovery, PKCE, ID token validation. |
| Rate limits | `governor` keyed limiters | GCRA, bounded memory with `retain_recent`. |
| Ids | `uuid` v7; endpoint ids from `rand` `OsRng` | Time-sortable capture ids; CSPRNG endpoint ids. |
| Assets | `rust-embed` | Single binary. |
| Allocator | `mimalloc` | musl's allocator is slow under threads (see §13). |

## 5. Capture pipeline

| Step | Behaviour |
|---|---|
| Routing | First path segment matching `^[0-9a-hjkmnp-tv-z]{24}$` exactly is a capture, on every method. All UI and API roots are shorter or contain other characters (§10), so there is no overlap. Uppercase or wrong length answers 404. |
| Early checks | Per-IP limiter, endpoint existence `SELECT` (410 if missing/expired/deleted), `Content-Length` above the limit answers 413 before reading. |
| Body | Stream through `http_body_util::Limited` (default 10 MiB, 413 on overflow). Bytes are reserved from a per-replica memory budget semaphore (default 256 MiB); when exhausted the capture waits up to 2 s, then 503. Body read timeout 30 s (slow senders). Bodies are stored as received; `Content-Encoding` is not decoded. |
| Early errors | Any response sent before the body is fully read carries `Connection: close` so a half-read HTTP/1.1 connection is never reused. |
| Headers | Stored as an ordered JSON array of `{name, value}` exactly as received, duplicates kept; non-UTF-8 values stored lossily. Header count and size limits are hyper's (100 headers, 64 KiB). |
| Query, form | Query parsed to an ordered `{name, value}` list. `application/x-www-form-urlencoded` parsed to fields; `multipart/form-data` parsed with `multer` into text fields and file descriptors (name, filename, content type, size), file content stays in the raw body. |
| Client IP | `WEBHOOKS_LIVE_TRUSTED_PROXIES` (CIDR list). If the socket peer is trusted, walk `X-Forwarded-For` right to left, skip trusted hops, take the first untrusted address; otherwise the peer. `X-Forwarded-Proto` and `X-Forwarded-Host` are read only from trusted peers and build the stored URL. `Forwarded` is ignored. |
| Ids | Capture `id` = UUIDv7 generated before the transaction; `seq` from the endpoint row (§2.1). |
| Store | Single transaction (§2.1) with `SET LOCAL statement_timeout = '2s'` and a 1 s pool acquire timeout. |
| Response | Status: override from `/{id}/{100..599}` first segment, else configured. Configured content type, headers and body. Always: `X-Request-Id`, `X-Endpoint-Id`, `Content-Security-Policy: sandbox`, `X-Content-Type-Options: nosniff`, `Cache-Control: no-store`. No rate-limit headers on success. |
| CORS mode | Every response adds `Access-Control-Allow-Origin: *` and `Access-Control-Expose-Headers: X-Request-Id, X-Endpoint-Id`. A preflight (`OPTIONS` with `Origin` and `Access-Control-Request-Method`) is captured and answered 204 with the requested method and headers echoed, `Access-Control-Max-Age: 600`. With CORS off, `OPTIONS` is an ordinary capture. |
| Delay | After commit (so the capture is stored and announced), `tokio::time::sleep`; no thread is held. Concurrent delayed responses per replica are capped (default 2000); over the cap the response is sent without delay and counted. |
| Store slow or down | Timeout or pool error answers 503 `Retry-After: 5`, `X-Webhooks-Live-Error: store_unavailable`; nothing is buffered or retried in memory, so limits and durability stay truthful. |
| Storage full | Each replica refreshes `sum(bytes_used)` every 10 s; above 110 % of the cap captures answer 507 until eviction catches up. |

## 6. Forwarding worker

**Decision: a Postgres job queue (`forward_jobs`) claimed with `FOR UPDATE SKIP LOCKED` by a worker loop in
every replica, with an SSRF-guarded `reqwest` client.** Jobs are inserted in the capture transaction, so a
stored capture always has its jobs, they survive restarts, and any replica can run them. Rejected: an
in-memory queue (lost on restart, single replica) and a Redis stream (extra service).

| Aspect | Behaviour |
|---|---|
| Claim | `UPDATE forward_jobs SET state='running', locked_until=now()+60s, attempts=attempts+1 WHERE id IN (SELECT id ... WHERE state IN ('pending','running') AND next_attempt_at <= now() AND (locked_until IS NULL OR locked_until < now()) ORDER BY next_attempt_at LIMIT n FOR UPDATE SKIP LOCKED) RETURNING *`. Woken by `pg_notify('whl_forward_wake')` from the capture transaction, polls every 2 s otherwise. An expired lease is reclaimed. |
| Request | Method: rule override or captured. URL: rule target, plus captured sub-path and query when `append_path`. Body: captured bytes. Headers: captured headers on the allow-list (case-insensitive), `Content-Type` (override or captured), `User-Agent: webhooks.live/<version>` unless allow-listed, `X-Webhooks-Live-Capture-Id`. Hop-by-hop headers, `Host`, `Content-Length`, `Transfer-Encoding` are never copied. |
| Retries | Retry on connect error, timeout, 408, 429, 5xx. Three attempts total by default, backoff 10 s then 60 s with ±20 % jitter, `Retry-After` honoured up to 5 min. Other statuses are final. |
| Result | Each attempt is a row in `forward_attempts` (status, duration, error class and message, final URL, response content type, first 4 KiB of response). Job end emits `forward_result` on `whl_events`. |
| Timeouts | 15 s per attempt; response read capped at 64 KiB. |
| Concurrency | Per replica: 32 jobs in flight, 4 per target host (`Semaphore` map). |
| SSRF guard | Custom `reqwest::dns::Resolve` resolves, drops every refused address, and hands only vetted addresses to the connector, so the checked address is the connected one (no rebinding). Literal-IP URLs are checked the same way. IPv4 refused: `0.0.0.0/8`, `10/8`, `100.64/10`, `127/8`, `169.254/16`, `172.16/12`, `192.0.0/24`, `192.168/16`, `198.18/15`, `224/4`, `240/4`, `255.255.255.255`, and `168.63.129.16/32` (Azure WireServer). IPv6: IPv4-mapped (`::ffff:0:0/96`), NAT64 (`64:ff9b::/96`) and 6to4 (`2002::/16`) are judged by their embedded IPv4; `64:ff9b:1::/48` and IPv4-compatible `::/96` are refused wholesale; every other address outside `2000::/3` is refused. `WEBHOOKS_LIVE_FORWARD_ALLOW_CIDRS` re-opens ranges. Schemes `http`/`https` only. |
| Redirects | Automatic redirects off; the worker follows up to 5 hops itself, running each hop through the same resolver. `https` to `http` is refused (`invalid_redirect`); `http` to `https` is allowed. 307/308 keep method and body; 301/302/303 turn any method other than `GET`/`HEAD` into `GET` without body. When a hop changes origin (scheme, host or port), `Authorization`, `Cookie`, `Cookie2` and `Proxy-Authorization` are dropped. |
| Proxy | System proxy settings ignored (`no_proxy`), because a proxy would resolve names itself and bypass the guard. |
| Rule validation | Rules are created and edited one at a time (api.md §5.3). On save the target must parse as an `http`/`https` URL; the host names `localhost`, `*.localhost`, `*.internal` and `*.local` are refused; the host must resolve to at least one address outside the refused ranges. Failures answer 422 with a field error on `target_url` (`target_refused` or `target_unresolvable`). Local targets are reached only through IP literals inside `WEBHOOKS_LIVE_FORWARD_ALLOW_CIDRS`. The check is repeated on every attempt and redirect, because DNS can change after save. |
| Manual resend | API creates a `manual` job with the same save-time check and the same guard; it may name a `rule_id` to reuse that rule's settings ("test with latest capture"). The result arrives as `forward_result`. |

## 7. Auth

**Decision: `WEBHOOKS_LIVE_AUTH_MODE=open|sso|mixed`. In `sso` and `mixed`, OIDC authorization code with PKCE against the operator's
issuer; server-side sessions in Postgres behind an opaque cookie.** Server-side sessions work across replicas
without shared keys, can be revoked, and keep tokens out of the browser. Rejected: signed JWT cookies
(no revocation, key rotation across replicas) and keeping the Envoy `SecurityPolicy` (it cannot leave the
capture path public while also knowing users for ownership and sharing).

| Aspect | Behaviour |
|---|---|
| Capture path | Always public, in both modes. |
| `open` | No users. Possessing the endpoint id grants full access to it. `GET /api/v1/endpoints` (listing) is refused; "my endpoints" live in browser storage (E6). |
| Login | `/auth/login?return_to=` stores `state`, PKCE verifier and nonce in `login_states` (10 min), redirects to the issuer. `/auth/callback` validates state, exchanges the code, validates the ID token (issuer, audience, nonce, signature via discovery JWKS), upserts `users` by `(issuer, subject)`. `return_to` must be a relative path starting with `/` and not `//`. |
| Session | 32 random bytes in cookie `__Host-whl_session` (`Secure; HttpOnly; SameSite=Lax; Path=/`); the database stores SHA-256 of it. Idle timeout 12 h, absolute 7 d, `last_seen_at` refreshed at most once a minute. IdP tokens are discarded after login. Logout deletes the row. |
| Ownership | `endpoints.owner_id` = creator. Shares in `endpoint_members` with `viewer` or `editor`. Shared by email of a user who has signed in at least once and whose email the IdP marked verified (`users.email_verified`); the match ignores case, and when two subjects carry one verified email the most recent login wins. Endpoints with no owner (`owner_id` null) have no role for anyone in `sso`. |
| Roles | Owner: everything. Editor: update response and forwarding, delete captures, resend. Viewer: read and stream. Endpoint delete and member management: owner only. Non-members get the same 410 `endpoint_gone` as an unknown id, never 403, so ids do not leak. |
| Who may sign in | Decided by the IdP client (GlobalArt SSO group enforcement); webhooks.live trusts any valid ID token from the issuer. |
| Machine clients | Personal API tokens with `Authorization: Bearer` (§19). Mode `mixed` and the `Caller` model: §17. |

## 8. Abuse control

| Limit | Default per replica | Key | On exceed |
|---|---|---|---|
| Capture per IP | 30/s, burst 60 | client IP | 429 `Retry-After` |
| Capture per endpoint | 20/s, burst 100 | endpoint id | 429 `Retry-After` |
| Management API per IP | 20/s, burst 40 | client IP | 429 JSON, `Retry-After` |
| Endpoint creation per IP | 10/min | client IP | 429 |
| Stored captures | 500 per endpoint | endpoint | 410 or ring eviction (C10) |
| Total storage | `WEBHOOKS_LIVE_STORAGE_CAP_BYTES` | global | eviction, then 507 at 110 % |

Limits are in-process `governor` limiters, so with N replicas and a non-sticky balancer the effective
ceiling is up to N times the configured value; the Helm values set per-replica numbers as target / N. This
is accepted: the per-endpoint capture cap and the global storage cap bound damage regardless of replica
count, and hard blocking belongs to the gateway (A3), fed by `webhooks_live_rate_limited_total` and the log field
`client_ip`. Rejected: a shared limiter in Postgres or Redis (a database round trip on every request to
enforce a soft limit).

**Maintenance job**: every 60 s, on the replica that wins `pg_try_advisory_lock`, in order: purge
soft-deleted endpoints (batches of 50); soft-delete endpoints with `last_active_at` older than retention;
while `sum(bytes_used)` exceeds the cap, soft-delete least recently active endpoints until it is at 90 %;
delete expired sessions and login states; hourly counter reconcile. Sliding expiry: captures update
`last_active_at` in the capture transaction; API reads and stream connects update it when older than 60 s.

## 9. Security

| Surface | Control |
|---|---|
| Stored XSS, raw body | `GET .../body` always answers `application/octet-stream`, or `text/plain; charset=utf-8` when `?view=text`, with `X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox; default-src 'none'`, `Cross-Origin-Resource-Policy: same-origin`, and `Content-Disposition: attachment` when `?download=1`. The captured `Content-Type` is never echoed. The UI renders bodies as text through React escaping, never `dangerouslySetInnerHTML`; captured HTML and SVG are shown as highlighted text and never rendered. Image preview is limited to raster types and uses `blob:` URLs created with the detected image MIME type. |
| Stored XSS, capture response | The configured response is served from the UI origin, so every capture response carries `Content-Security-Policy: sandbox` and `nosniff`: the document gets an opaque origin, no cookie or same-origin API access. |
| Header injection | Configured response headers: at most 20; name an RFC 9110 token up to 64 bytes; value visible ASCII, space and tab, up to 1024 bytes, no CR, LF or NUL. A name repeated case-insensitively is rejected (`duplicate`); this applies to the configured response only, captured request headers keep their duplicates. Denied names: `Set-Cookie`, `Set-Cookie2`, `Cache-Control`, `Content-Encoding`, `Content-Length`, `Transfer-Encoding`, `Connection`, `Upgrade`, `TE`, `Trailer`, `Keep-Alive`, `Proxy-*`, `Strict-Transport-Security`, `Clear-Site-Data`, `Alt-Svc`, `Refresh`, `Content-Security-Policy`, `Content-Security-Policy-Report-Only`, `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, `Permissions-Policy`, `Service-Worker-Allowed`, `Public-Key-Pins`, `Public-Key-Pins-Report-Only`, `Report-To`, `NEL`, `Origin-Trial`, `Document-Policy`, `WWW-Authenticate`, `Proxy-Authenticate`, `Accept-CH`, `Critical-CH`, `Set-Login`, `Access-Control-*`, `Cross-Origin-*`. `Set-Cookie` is denied because a capture response could toss a `__Host-` session cookie on the shared origin; `WWW-Authenticate` because a native browser login prompt on our origin would collect credentials for the endpoint owner; the rest because they change origin-wide browser state or the protections the server sets. Violations of all headers are reported together, up to 50 field errors. |
| Content type | Parsed as a MIME type (`type/subtype` plus parameters), up to 255 bytes, no control characters; a trailing `;`, with optional spaces around it, is accepted. |
| SSRF | §6. |
| Smuggling and parsing | Hyper rejects conflicting `Content-Length`/`Transfer-Encoding`, obs-fold and malformed chunking; webhooks.live only listens behind Envoy; early responses close the connection (§5); the forwarder builds fresh requests and never relays raw bytes or hop-by-hop headers. |
| CSRF | No CORS on `/auth`; `/api` allows token clients only, without credentials (§19). In `sso`, mutating requests carrying the session cookie must have `Origin` equal to `WEBHOOKS_LIVE_PUBLIC_URL`'s origin (else 403 `csrf_rejected`); request bodies must be `application/json` (else 415); cookie is `SameSite=Lax`. In `open` there is no ambient credential, so no CSRF surface. |
| Id leakage | UI sends `Referrer-Policy: no-referrer`; logs carry endpoint and capture ids but never header values or bodies. |
| UI headers | `Content-Security-Policy: default-src 'none'; script-src 'self' 'sha256-<theme script>'; style-src 'self'; style-src-attr 'unsafe-inline'; img-src 'self' data: blob:; worker-src 'self'; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`, `X-Frame-Options: DENY`. The hash is that of the single inline theme script in `index.html`, computed by `whl-web` at server start from the embedded `index.html`, so it cannot drift. `blob:` serves typed image previews, `worker-src` the pretty-printing workers, `connect-src 'self'` covers the API and SSE. |
| `style-src-attr 'unsafe-inline'` | Needed because the virtual list positions rows with inline `style` attributes. Acceptable because the UI never inserts untrusted markup as HTML (React text nodes and CodeMirror text only, no `dangerouslySetInnerHTML`, no rendered HTML or SVG), so no attacker-controlled element can carry a `style` attribute; scripts stay limited to `'self'` and one hash. |

## 10. Frontend delivery and path layout

**Decision: the built React app is embedded in the binary with `rust-embed` and served by `whl-web` on
the same origin.** One artefact, UI and API versions never skew, no CORS. Rejected: a separate static
server or container (two images to version, plus cross-origin cookies).

| Path | Served |
|---|---|
| `/{endpoint_id}`, `/{endpoint_id}/{*rest}` | Capture (24 chars, Crockford base32 lowercase) |
| `/api/v1/...` | Management API, `/api/v1/endpoints/{id}/stream`, `/api/v1/inbox/stream`, `/api/v1/openapi.json` (§19) |
| `/a/{alias}`, `/a/{alias}/{*rest}` | Capture through an alias (§24) |
| Host `{endpoint_id}.<subdomain capture host>` | Capture through a subdomain, any path (§24) |
| `/auth/login`, `/auth/callback`, `/auth/logout` | OIDC |
| `/assets/{file}` | Hashed build output: `Cache-Control: public, max-age=31536000, immutable` |
| `/`, `/e/{endpoint_id}`, `/e/{endpoint_id}/c/{capture_id}`, `/e/{endpoint_id}/settings`, `/e/{endpoint_id}/forwarding`, `/e/{endpoint_id}/diff/{a}/{b}`, `/endpoints`, `/signin`, `/settings` | `index.html`, `Cache-Control: no-cache`, UI headers and Content-Security-Policy of §9; the list follows ui-design.md §2 |
| `/favicon.ico`, `/robots.txt` | Static, `max-age=86400`; `robots.txt` disallows everything |
| anything else | 404 |

Endpoint id: 15 bytes from `OsRng` (120 bits, on par with UUID v4's 122), Crockford base32 lowercase
without padding, exactly 24 characters (alphabet `0-9a-z` minus `i l o u`). No UI or API root reaches 24
characters, and deep links use the `/e/` prefix, so a root segment is either a capture id or a fixed route.
The UI is fetched from `index.html` for known UI prefixes only; unknown paths stay 404 so typos in capture
URLs are visible to senders. The release image build fails when `web/dist/index.html` is missing (the
`Containerfile` checks it); debug builds read assets from disk.

## 11. Observability and operations

| Item | Decision |
|---|---|
| Ports | `8080` public; `9090` admin (`/healthz`, `/readyz`, `/metrics`), not routed by the gateway. |
| `/healthz` | Process alive. |
| `/readyz` | 200 after the first database connection and schema-version check succeed, 503 once shutdown starts. It does not follow later database outages: they hit every replica alike, and dropping pods from the balancer would only replace webhooks.live's 503 with Envoy's. |
| Schema check | On start the binary compares applied migrations with embedded ones and refuses to start if the database is behind. |
| Logs | JSON to stdout: `ts`, `level`, `msg`, `target`, `route`, `method`, `status`, `duration_ms`, `client_ip`, `endpoint_id`, `capture_id`. `WEBHOOKS_LIVE_LOG` uses `EnvFilter` syntax. |
| Shutdown | SIGTERM: `/readyz` 503, wait `WEBHOOKS_LIVE_SHUTDOWN_DRAIN` (5 s) for Envoy to deregister, stop accepting, send `bye` to streams, let in-flight captures (including delays) finish, forward workers stop claiming and finish or release their current attempt; hard stop at `WEBHOOKS_LIVE_SHUTDOWN_GRACE` (25 s), below `terminationGracePeriodSeconds: 30`. |

| Metric | Type | Labels |
|---|---|---|
| `webhooks_live_captures_total` | counter | `result` = stored, gone, full, too_large, rate_limited, store_unavailable, storage_full |
| `webhooks_live_capture_duration_seconds` | histogram | (excludes delay) |
| `webhooks_live_capture_body_bytes` | histogram | |
| `webhooks_live_body_budget_bytes_in_use` | gauge | |
| `webhooks_live_delayed_responses` | gauge | |
| `webhooks_live_http_requests_total`, `webhooks_live_http_request_duration_seconds` | counter, histogram | `route`, `status` |
| `webhooks_live_rate_limited_total` | counter | `scope` = capture_ip, capture_endpoint, api_ip, create_ip |
| `webhooks_live_sse_connections` | gauge | |
| `webhooks_live_sse_events_total` | counter | `event` |
| `webhooks_live_sse_resyncs_total` | counter | `reason` = lagged, listener_reconnect, gap |
| `webhooks_live_listener_reconnects_total` | counter | |
| `webhooks_live_forward_attempts_total` | counter | `outcome` = success, http_error, timeout, connect_error, blocked |
| `webhooks_live_forward_duration_seconds` | histogram | |
| `webhooks_live_forward_queue_depth` | gauge | |
| `webhooks_live_store_bytes`, `webhooks_live_endpoints` | gauge | |
| `webhooks_live_evictions_total` | counter | `reason` = expired, cap, deleted |
| `webhooks_live_db_pool_connections` | gauge | `state` = idle, busy |
| `webhooks_live_auth_available` | gauge | `sso` only: 1 once OIDC discovery has succeeded, 0 while the identity provider is unreachable (`/auth/login` and `/auth/callback` answer 503 `auth_unavailable`) |

| Env | Default |
|---|---|
| `WEBHOOKS_LIVE_PUBLIC_URL` | required |
| `WEBHOOKS_LIVE_DATABASE_URL`, `WEBHOOKS_LIVE_DB_POOL_SIZE` | required, 20. `whl-core` enforces a minimum of 2; use at least 3 in practice, because migrations need 2 connections and the listener holds 1 for its lifetime |
| `WEBHOOKS_LIVE_LISTEN_ADDR`, `WEBHOOKS_LIVE_ADMIN_ADDR` | `0.0.0.0:8080`, `0.0.0.0:9090` |
| `WEBHOOKS_LIVE_AUTH_MODE` | `open`; `open`, `sso` or `mixed` (§17) |
| `WEBHOOKS_LIVE_OIDC_ISSUER`, `WEBHOOKS_LIVE_OIDC_CLIENT_ID`, `WEBHOOKS_LIVE_OIDC_CLIENT_SECRET`, `WEBHOOKS_LIVE_OIDC_SCOPES` | issuer, client id and secret required in `sso` and `mixed` (the server refuses to start without them); scopes `openid email profile` |
| `WEBHOOKS_LIVE_OIDC_PROVIDER_NAME` | unset; shown on the sign-in button, the UI uses a neutral label when unset |
| `WEBHOOKS_LIVE_RING_MODE_ALLOWED` | true; false rejects `overflow: ring` |
| `WEBHOOKS_LIVE_SESSION_IDLE`, `WEBHOOKS_LIVE_SESSION_MAX` | `12h`, `7d`; the maximum is capped at 90 days and the idle timeout must not exceed it |
| `WEBHOOKS_LIVE_AUTH_MAX_PENDING_LOGINS` | 50000; `/auth/login` answers 503 `login_busy` while that many unexpired sign-ins are pending |
| `WEBHOOKS_LIVE_TRUSTED_PROXIES` | empty |
| `WEBHOOKS_LIVE_RETENTION`, `WEBHOOKS_LIVE_MAX_CAPTURES`, `WEBHOOKS_LIVE_MAX_BODY_BYTES` | `7d`, 500, 10 MiB |
| `WEBHOOKS_LIVE_BODY_MEMORY_BUDGET`, `WEBHOOKS_LIVE_MAX_DELAYED` | 256 MiB, 2000 |
| `WEBHOOKS_LIVE_STORAGE_CAP_BYTES` | 20 GiB |
| `WEBHOOKS_LIVE_RATE_CAPTURE_IP`, `WEBHOOKS_LIVE_RATE_CAPTURE_ENDPOINT`, `WEBHOOKS_LIVE_RATE_API_IP`, `WEBHOOKS_LIVE_RATE_CREATE_IP` | `30/s:60`, `20/s:100`, `20/s:40`, `10/m:10` |
| `WEBHOOKS_LIVE_FORWARD_ENABLED`, `WEBHOOKS_LIVE_FORWARD_CONCURRENCY`, `WEBHOOKS_LIVE_FORWARD_MAX_ATTEMPTS`, `WEBHOOKS_LIVE_FORWARD_ALLOW_CIDRS` | true, 32, 3, empty |
| `WEBHOOKS_LIVE_SHUTDOWN_DRAIN`, `WEBHOOKS_LIVE_SHUTDOWN_GRACE` | `5s`, `25s` |
| `WEBHOOKS_LIVE_LOG`, `WEBHOOKS_LIVE_CONFIG` | `info`, unset |

## 12. Packaging and deploy

| Item | Decision |
|---|---|
| Binary | `x86_64-unknown-linux-musl`, static, rustls only, assets embedded, `mimalloc`. Meets O1 literally. |
| Image | `Containerfile`: node stage builds `web/dist`, Rust stage builds `webhooks-live`, final `gcr.io/distroless/static-debian13:nonroot` (CA bundle included). Tag `app-<yyyymmddHHMMSS>-<sha>`, pushed by `ci-cd/docker-build.yml` to `images.globalart.hu/prod/webhooks-live`, the registry the current gitops already uses. |
| Chart | House `charts/app` (gitops `./charts/app`, 1.14.0): Deployment `webhooks-live` ×2, `site: moscow`, PDB `minAvailable: 1`, requests 100m / 256Mi, memory limit 768Mi (body budget 256 MiB plus headroom), probes on 9090, `terminationGracePeriodSeconds: 30`. `jobs.migrate` runs `webhooks-live migrate up` from the same image as a pre-install/pre-upgrade hook. Secrets (`WEBHOOKS_LIVE_DATABASE_URL`, `WEBHOOKS_LIVE_OIDC_CLIENT_SECRET`) in a SOPS file. |
| Route | One `HTTPRoute` on `moscow-gateway` section `https`: regex `^/api/v1/endpoints/[^/]+/stream$` with `timeouts.request: 0s`; everything else `PathPrefix /` with `timeouts.request: 60s`. No `SecurityPolicy`: webhooks.live does OIDC itself. `WEBHOOKS_LIVE_TRUSTED_PROXIES=172.17.0.0/16` (Envoy pods, as in the current nginx config). |
| Flux | `HelmRelease` plus `ImagePolicy webhooks-live-app` on `^app-\d{14}-[0-9a-f]+$`, same as other apps. |
| Database | Own database and role on a prod1 Postgres cluster (open question 1). Excluded from long-term backups: captures are ephemeral. |
| Multi-replica | Any replica serves any request; no sticky sessions; scale by replica count. Rate limits are per replica (§8). |

**Cutover** (no data migration; the id formats differ, so old and new capture URLs never collide):

1. Deploy webhooks.live in namespace `webhooks-live-prod` at `webhooks-live.globalart.hu` (wildcard `globalart-hu-tls`); add
   `https://webhooks-live.globalart.hu/auth/callback` and `https://webhooks.globalart.hu/auth/callback` to the
   OIDC client. Run side by side for a week; check metrics, load test, e2e.
2. Switch `webhooks.globalart.hu` to webhooks.live in one gitops commit: webhooks.live's route takes the hostname; the old
   `webhooks-capture` route (UUID regex) keeps pointing at the old app, so senders configured with old URLs
   keep landing for 7 days; the old UI moves to `webhooks-old.globalart.hu` behind its existing SSO policy.
3. After 7 days delete the old app, echo server, Redis, routes, `SecurityPolicy`, `ImagePolicy webhooks-app`.
4. Rollback at any point: revert the switch commit.

## 13. Testing

| Level | Scope | How |
|---|---|---|
| Unit | `whl-core` rules, client IP resolution, status override, header validation, SSRF range checks, cursor encoding, SSE framing | `cargo test`, one case per rule |
| Integration | Every API route, capture path end to end, limits and ring mode, concurrent captures keep `seq` gapless, SSE replay and `resync`, two app instances on one database deliver events across instances, forward worker against a local target with a test-only allow-list, eviction | `whl-testkit` creates a fresh database per test on `WEBHOOKS_LIVE_TEST_DATABASE_URL`; `tools/test-db.sh` starts `postgres:17` with podman. CI runs a Postgres service container. |
| Load | Capture path: 2 replicas, 300 req/s of 2 KiB POSTs over 50 endpoints for 5 min, p99 < 50 ms and zero 5xx, with 20 SSE viewers including one that never reads; 10 MiB bodies at 5 req/s keep memory under the limit | `oha` scripts in `tools/loadtest/`, run against the side-by-side deployment |
| E2E | Five Playwright scenarios: create endpoint and see a curl capture arrive live; status override and custom response; forward result shown; `sso` login and a share between two users; raw body of an HTML capture does not execute | `e2e/`, against a podman-composed webhooks.live, Postgres and a mock OIDC issuer |

## 14. Delivery plan

Risky milestones go to `coder-pro` and get an `xhigh` review.

| # | Milestone | Done when | Risk |
|---|---|---|---|
| M0 | Workspace skeleton: crates, `figment` config, JSON tracing, admin listener with probes and `/metrics`, graceful shutdown frame, `Containerfile`, CI fmt/clippy/test | `webhooks-live serve` starts, probes answer, image builds | – |
| M1 | Schema migrations, `webhooks-live migrate`, schema-version check, `whl-testkit` with podman Postgres | migrations up/down in tests; server refuses an old schema | – |
| M2 | `whl-core`: id generation/parsing, status override, response header and content-type validation | unit tests for every rule in §9 | security |
| M3 | Management API, `open` mode: instance, endpoint CRUD, error body, rate-limit headers stub | integration test per route in api.md | – |
| M4 | Capture ingest: routing, early checks, body limit and memory budget, client IP, parsing, capture transaction, ring mode, 404/410/413/503/507 | integration tests incl. 50 concurrent captures yield gapless `seq` | network input, concurrency |
| M5 | Serving untrusted content: configured response, override, delay, CORS, capture response headers, raw body endpoint headers | exact header sets asserted per api.md | security |
| M6 | Capture read API: list with cursor, filters and search, detail, latest, deletes | integration tests per filter | – |
| M7 | Live: `pg_notify`, listener, hub, SSE route with replay, `resync`, pings, lifetime, `bye` | two app instances on one database deliver events; lagging viewer gets `resync` | concurrency |
| M8 | `sso`: OIDC, sessions, origin check, roles on every route, members API, endpoint listing | tests against a mock issuer; non-member gets 410 `endpoint_gone` | security |
| M9 | Rate limiters, storage cap flag, maintenance job, sliding expiry, purge | eviction and cap tests; job runs on one instance only | concurrency |
| M10 | SSRF-guarded client: resolver, ranges, redirects, timeouts | unit tests per range and redirect case | security, network |
| M11 | Forward queue: rule routes with save-time target check, jobs in capture transaction, claim loop, retries, attempts, manual resend, `forward_result` | integration tests incl. lease reclaim | concurrency |
| M12 | `whl-web`: embedding, cache headers, UI security headers with the theme-script hash computed at start from the embedded `index.html`, SPA fallback for the routes of ui-design.md §2, `Containerfile` check for `web/dist/index.html` | route table of §10 asserted; CSP hash matches the served script | – |
| M13 | Deploy: gitops HelmRelease, route, ImagePolicy, migrate job, OIDC client URIs, `webhooks-live.globalart.hu` | side-by-side instance live | deploy (`xhigh` review) |
| M14 | Load scripts and Playwright scenarios | targets of §13 met on the side-by-side instance | – |
| M15 | Cutover steps 2–3 | old stack removed | deploy |

The React UI itself is planned from `ui-design.md`; it can start after M3 against api.md. Parity milestones
M16–M48 continue in §35.

## 15. Open questions for the owner

1. Which prod1 Postgres cluster hosts the `webhooks_live` database, and the storage cap (20 GiB assumed).
2. Do pods in `webhooks-live-prod` have direct internet egress for forwarding? If prod1 requires the egress proxy,
   forwarding needs a CONNECT-to-vetted-IP mode (one more milestone) to keep the SSRF guard intact.
3. Resolved: personal API tokens are part of the parity plan (§19, M20).
4. Keep the OPS OIDC client (Ops group only, as today) or create a dedicated webhooks.live client with wider access?

## 16. Parity scope and crates

Sections 16–36 design the parity plan ([parity.md](parity.md), product-spec §13–§28). The shape of §1 holds:
one binary, Postgres as the only datastore, identical replicas. Two new listeners (SMTP, DNS) and four new
worker loops (rules, notifications, schedules, export streaming) live in the same binary.

**Decision: one binary with feature switches, not separate services.** Every new worker uses the §6 pattern
(`FOR UPDATE SKIP LOCKED`, leases, `pg_notify` wake-ups), so any replica runs any job and nothing new is
stateful. Rejected: a separate mail or DNS deployment (a second image and release cadence for code that shares
the store, limits and capture pipeline; can be split later by running the same image with only one listener
enabled).

| Crate | Owns | Depends on |
|---|---|---|
| `whl-template` | Template parser, AST, renderer, filters, functions, bounded writer, error positions. No I/O. | `whl-core` |
| `whl-rules` | Rule and step types with validation, condition evaluation, plan cut (§22), executor over seams (`Fetch`, `Vars`, `Enqueue`, `Script`) | `whl-core`, `whl-template` |
| `whl-script` | Rhai engine configuration, limits, allocation accounting, script thread pool | `whl-core` |
| `whl-secrets` | Keyring, seal and open, capture-auth MAC, redaction | `whl-core` |
| `whl-quota` | Tier table, in-process limiter registry, durable usage counters API | `whl-core`, `whl-store` |
| `whl-notify` | Channel adapters, message rendering, delivery worker, Web Push, SMTP submission | `whl-forward`, `whl-store`, `whl-template`, `whl-secrets`, `whl-quota` |
| `whl-schedule` | Cron validation, claim loop, run to capture | `whl-forward`, `whl-store`, `whl-template`, `whl-quota` |
| `whl-export` | Streaming JSON, NDJSON, CSV, ZIP writers; import reader | `whl-store` |
| `whl-mail` | SMTP listener, STARTTLS, message to capture, part decoding | `whl-core`, `whl-store`, `whl-quota` |
| `whl-dns` | UDP and TCP responder, answer rules, response rate limiting, query to capture | `whl-core`, `whl-store`, `whl-quota` |
| `whl-cli` | Binary `webhooks-live-cli` (forwarder) | `whl-core` only; own DTOs |

| Existing crate | Gains |
|---|---|
| `whl-core` | Tier and quota types, PROXY protocol v2 parser, alias and tag validation, capture kind mapping |
| `whl-store` | Every table of §20, queries for the new workers |
| `whl-capture` | Alias and subdomain routing, sender allow list, capture auth, signature verification, templated response, rules hook (§22), hop guard |
| `whl-auth` | `Caller` (anonymous, session, token), `mixed` role resolution, tokens, view grants |
| `whl-api` | All new routes, Bearer extractor, API CORS, OpenAPI document |
| `whl-forward` | `OutboundClient` shared by every outbound purpose (§26), vendor proxy mode |
| `whl-live` | Combined inbox stream, new events |
| `whl-server` | Listener and worker wiring, `secrets rotate` and `users set-tier` commands |

## 17. Mixed mode, callers, tiers and quotas

**Decision: a third auth mode `mixed`, one `Caller` type for every request, and tiers that scale limits.**
Production is public (`open`), but tokens, aliases, schedules, never-expire and email notifications need a
person to own them and to answer for abuse. `mixed` keeps anonymous use exactly as today and adds optional
OIDC sign-in. Rejected: two deployments (one `open`, one `sso`), which would split URLs, data and limits.

| Endpoint | Caller | Role |
|---|---|---|
| Anonymous (`owner_id` null) | anyone holding the id (session, token or none) | `owner`; person-only features still require a signed-in caller (403 `sign_in_required`) |
| Owned | owner (session or token) | `owner` |
| Owned | member | member's role |
| Owned | anyone else | none: 410 `endpoint_gone` (§7) |

| Aspect | Behaviour |
|---|---|
| `Caller` | `Anonymous { ip }`, `Session { user, ip }`, `Token { user, token, ip }`. Resolved once per request by an extractor in `whl-auth`; handlers ask `role_on(endpoint)`, which feeds the existing `decide_access`. A token's role is further capped by its scopes (§19). |
| Claim | `POST /api/v1/endpoints/{id}/claim`, signed in, `mixed` only. One transaction locks the live endpoint row (`FOR UPDATE`) and, while `owner_id IS NULL`, requires the proof below, then sets `owner_id = $user`, clears `creator_net` and deletes any `endpoint_members` rows. Two parallel claims: the row lock lets exactly one win. Answers: 403 `sign_in_required` (anonymous caller), 403 `claim_refused` (proof fails), 409 `already_owned` (the caller is the owner or a member), 410 `endpoint_gone` (unknown, expired, deleted, or owned by someone else: a stranger learns nothing about owned endpoints). A view password is dropped on claim (roles replace it). |
| Claim proof | The caller holds the id, is signed in, the caller's client address (resolved like the rate limiter, trusted proxies only) lies in the stored `creator_net`, and the endpoint was created less than 24 hours ago (`CLAIM_WINDOW`, a constant of `whl-api`). A missing stored network or an unknown caller address refuses. |
| Member routes | In `mixed`, `/members` routes need a signed-in owner of an owned endpoint: anonymous callers get 403 `sign_in_required`, a signed-in caller on an ownerless endpoint gets 409 `claim_required`, so no member row can exist before ownership. |
| Listing | `GET /api/v1/endpoints` lists the owned and shared endpoints of the signed-in user; ownerless endpoints are never listed. Anonymous callers: 403 `sign_in_required`. |
| Creator network | Anonymous endpoints created in `mixed` store `creator_net` (/24 for IPv4, /56 for IPv6, IPv4-mapped addresses count as IPv4), never the address itself; endpoints created by a signed-in user and endpoints created in `open` store none. `creator_net` is cleared when the endpoint is claimed. |
| `open`, `sso` | Unchanged. `sso` behaves as `mixed` without anonymous creation. |

Tiers are named limit sets in the config file (`[tiers.<name>]`); `anonymous` and `member` are built in, an
operator may add more and assign them with `webhooks-live users set-tier <email> <tier>` (`users.tier`).

| Limit | `anonymous` | `member` |
|---|---|---|
| Live endpoints | 20 per creator network | 200 per user |
| Captures per endpoint (max settable) | 500 | 10000 |
| Retention (max settable) | 7 d sliding | 365 d; never-expire on 10 endpoints |
| Storage per owner | endpoint caps only | 2 GiB |
| Rules per endpoint; steps per rule | 5; 10 | 25; 20 |
| HTTP steps before the response | 0 | 2 per run |
| Script time before / after the response | 50 ms / 200 ms | 100 ms / 1 s |
| Outbound requests (forward, rule, schedule, composer, webhook channel) | 200/h per endpoint, 1000/day per network | 2000/h per user |
| Notifications | 30/h per endpoint; webhook, Slack, Discord, Telegram | 300/h per user; plus email (50/day) and push |
| Email received | 30/h per endpoint | 600/h per endpoint |
| DNS queries stored | 60/min per endpoint | 600/min per endpoint |
| Schedules; minimum interval | none | 5 per user; 5 min |
| Aliases; API tokens | none | 5; 20 |
| Export size; exports | 256 MiB; 10/h | 2 GiB; 30/h |
| Import | none | 100 MiB; 10/day |

| Enforcement | Mechanism | Why |
|---|---|---|
| Request rates (capture, API, unlock, SMTP and DNS per source) | In-process `governor`, per replica (§8) | Cheap, soft, damage bounded elsewhere |
| Cost quotas (outbound, notifications, email and DNS stored, exports, imports, composer) | `usage_counters` conditional upsert in one statement before the action: `INSERT ... VALUES ($s, $m, $w, 1) ON CONFLICT (subject, metric, window_start) DO UPDATE SET count = usage_counters.count + 1 WHERE usage_counters.count < $limit RETURNING count`; no row means over the limit | Must hold across replicas and restarts; one round trip only on actions that already cost a network call |
| Counted objects (endpoints, rules, schedules, aliases, tokens, never-expire) | `SELECT count(*)` inside the creating transaction after `SELECT ... FROM users WHERE id = $1 FOR UPDATE` (anonymous: `pg_advisory_xact_lock` on the network) | No race between two parallel creates |
| Storage per owner | Maintenance job writes `users.bytes_used` every 60 s; each replica caches the over-quota set like the §5 storage flag; captures to an over-quota owner answer 507 | No extra row update on the capture path |

Subjects: `u:<user id>`, `e:<endpoint id>`, `n:<network>`, `c:<channel id>`. Windows: hour and day starts in UTC.
The maintenance job deletes windows older than 2 days.

## 18. Secrets

**Decision: per-endpoint secrets in Postgres, sealed with XChaCha20-Poly1305 (`chacha20poly1305`, RustCrypto)
under a keyring from `WEBHOOKS_LIVE_SECRETS_KEYS`.** Steps, channels, schedules and verifiers refer to secrets by
name; plaintext exists only in worker memory at the moment of use. Rejected: Kubernetes Secrets or a vault per
user secret (thousands of dynamic secrets, a new service); plaintext columns (a database dump leaks every
user's tokens).

| Aspect | Behaviour |
|---|---|
| Keyring | `id:base64(32 bytes)` list, comma-separated; the first is active. `webhooks-live secrets rotate` re-seals every row under the active key. Unset keyring: secrets, `secret.*` references, Telegram, S3 and verifiers are unavailable (`features.secrets: false`). |
| Seal | 24-byte random nonce; associated data `endpoint_id \|\| 0x00 \|\| name \|\| key_id`, so a ciphertext copied to another endpoint or name fails to open. |
| Limits | Name `^[A-Z][A-Z0-9_]{0,63}$`, value up to 4 KiB, 20 per endpoint. |
| API | Write-only: list returns names and timestamps, never values. |
| Use | Allowed in `http`, `forward` (header values), `store_s3`, notification configs, schedule URLs and headers, verifiers. Refused at save time in response steps and response templates (field code `secret_not_allowed`). |
| Redaction | Before any run log, delivery error or attempt detail is stored, every decrypted value of 4 or more bytes, and its base64 and percent-encoded forms, is replaced by `[secret:NAME]`. Values live in `Zeroizing` buffers. |
| Capture auth | Not stored as a secret: `capture_auth` keeps `HMAC-SHA-256(mac_key, value)` where `mac_key` is derived from the active key with HKDF (`whl-capture-auth`). Constant-time comparison; no decryption on the capture path. |
| View password | `argon2id` (19 MiB, t=2, p=1) in `endpoints.view_password_hash`. |
| Who can read | Editors can send a secret to a URL they control; secrets protect against viewers, exports and database leaks, not against editors. Stated in the UI. |

## 19. API tokens, API CORS, OpenAPI

**Decision: opaque personal tokens hashed with SHA-256, Bearer only, scopes capped by the user's role.**
Tokens carry 256 random bits, so a fast hash is enough and lookup is one primary-key read. Rejected: JWT access
tokens (no revocation without a deny list, key management); OAuth device flow for the CLI (an IdP feature the
operator may not have).

| Aspect | Behaviour |
|---|---|
| Format | `whl_` plus 52 characters of lowercase base32 (32 bytes from `OsRng`). Shown once. `prefix` = first 12 characters, kept for display. |
| Scopes | `read`, `endpoints:write`, `captures:write`, `rules:write`, `send`. `read` covers every GET and both streams. Optional `endpoint_ids` limit. |
| Effective right | `min(user's role on the endpoint, scopes)`. Token, member and ownership routes refuse tokens (403 `session_required`). |
| Extraction | `Authorization: Bearer` present: token path only, cookies ignored, CSRF origin check skipped (no ambient credential). Unknown, expired or revoked: 401 `invalid_token`, never a fallback to the cookie. |
| Bookkeeping | `last_used_at` written at most once a minute per token. Rate limiter keyed by token id with the API limits. |
| Expiry | Required, 1–365 days, default 90. The maintenance job deletes tokens 30 days after expiry or revocation. |
| Scope map | `GET`/`HEAD` and both streams: `read`. `PATCH`/`DELETE` of an endpoint, `POST /endpoints`, secret `PUT`/`DELETE`: `endpoints:write`. Capture `DELETE`: `captures:write`. Forward rule `POST`/`PATCH`/`DELETE`: `rules:write`. Capture forward and resend: `send`. Claim, members, tokens and every unlisted route or method: session only. A missing scope or an endpoint outside the limit answers 403 `insufficient_scope` before any endpoint is looked up. |
| Endpoint limit | A limited token sees only its endpoints in the listing, cannot create endpoints, and is refused on any other endpoint. |
| Lookup | `prefix` is indexed, not unique; every row with the prefix is compared with the SHA-256 in constant time, an unknown prefix costs the same comparison against a dummy, and the expiry and revocation checks run after it, so unknown, wrong, expired, revoked and malformed tokens share one 401 body. |
| Creation limits | At most 20 active tokens and 10 creations per hour per user, counted under the user row lock; 422 `limit_reached`, 429 `quota_exceeded`. |
| Rate limit | One limiter per token id with the `api_ip` rate, applied after the token resolves; a stream re-check does not count. |

**API CORS.** `/api/v1/*` answers preflights with `204`, `Access-Control-Allow-Origin: *`,
`Access-Control-Allow-Methods: GET, POST, PUT, PATCH, DELETE`, `Access-Control-Allow-Headers: Authorization,
Content-Type, Last-Event-ID`, `Access-Control-Max-Age: 600`; responses carry `Access-Control-Allow-Origin: *` and
expose `Location, RateLimit-Limit, RateLimit-Remaining, Retry-After`. `Access-Control-Allow-Credentials` is never
sent, so browsers never attach the session cookie cross-origin; the §9 origin check still guards cookie requests.
`/auth/*` has no CORS. Switch: `WEBHOOKS_LIVE_API_CORS`. This replaces "No CORS on `/api`" in §9.

**OpenAPI.** `utoipa` derives schemas from the DTOs and paths from annotated handlers; `whl-api` assembles the
document and serves `GET /api/v1/openapi.json` (OpenAPI 3.1, `ETag` = build version). A test compares the router's
route table with the document's paths, so an undocumented route fails CI. The reference page is a UI route
rendered by the app's own components (ui-design.md), with no third-party bundle. Rejected: a hand-written file
(drifts); a bundled third-party viewer (inline scripts against the §9 CSP).

## 20. Data model additions

Capture kinds share `captures`; kind-specific data goes to `meta`. The body column keeps the raw bytes of
whatever arrived (HTTP body, RFC 5322 message, response of a schedule run).

| Common field | `email` | `dns` | `schedule` |
|---|---|---|---|
| `method` | `SMTP` | query type (`A`, `TXT`, ...) | request method |
| `url` | `mailto:<recipient>` | `dns:<query name>` | target URL |
| `path` | `+tag` or empty | labels left of the id, or empty | empty |
| `host` | mail domain | DNS zone | target host |
| `client_ip` | SMTP client | resolver | connected target address |
| `headers`, `query` | message headers, `[]` | `[]`, `[]` | response headers, `[]` |
| body | raw message | empty | response body, first 1 MiB |
| `response_status` | 250 | 0 (NOERROR; only answered names are stored) | target status, 0 on transport error |
| `meta` | envelope from and to, HELO, TLS, subject, from, to, cc, date, message id, parts list | name, type, class, transport, DNS id, flags, EDNS size, client subnet | schedule id, request summary, duration, final URL, error, truncated |

```sql
ALTER TABLE users
  ADD COLUMN tier           text NOT NULL DEFAULT 'member',
  ADD COLUMN email_verified boolean NOT NULL DEFAULT false,
  ADD COLUMN bytes_used     bigint NOT NULL DEFAULT 0;

ALTER TABLE endpoints
  ADD COLUMN creator_net           cidr,
  ADD COLUMN tags                  text[] NOT NULL DEFAULT '{}' CHECK (cardinality(tags) <= 10),
  ADD COLUMN retention_seconds     integer CHECK (retention_seconds >= 3600),
  ADD COLUMN never_expires         boolean NOT NULL DEFAULT false,
  ADD COLUMN max_captures          integer CHECK (max_captures > 0),
  ADD COLUMN capture_ttl_seconds   integer CHECK (capture_ttl_seconds >= 60),
  ADD COLUMN sender_allow_cidrs    cidr[] NOT NULL DEFAULT '{}' CHECK (cardinality(sender_allow_cidrs) <= 20),
  ADD COLUMN capture_auth          jsonb,
  ADD COLUMN view_password_hash    text,
  ADD COLUMN response_templated    boolean NOT NULL DEFAULT false,
  ADD COLUMN verifier              jsonb,
  ADD COLUMN email_enabled         boolean NOT NULL DEFAULT false,
  ADD COLUMN dns_enabled           boolean NOT NULL DEFAULT false,
  ADD COLUMN rules_version         integer NOT NULL DEFAULT 0,
  ADD COLUMN rule_count            smallint NOT NULL DEFAULT 0,
  ADD COLUMN rules_sync            boolean NOT NULL DEFAULT false,
  ADD COLUMN capture_channel_count smallint NOT NULL DEFAULT 0;
CREATE INDEX endpoints_tags ON endpoints USING gin (tags);
CREATE INDEX endpoints_capture_ttl ON endpoints (id) WHERE capture_ttl_seconds IS NOT NULL;
CREATE INDEX endpoints_creator_net ON endpoints (creator_net) WHERE owner_id IS NULL AND deleted_at IS NULL;

ALTER TABLE captures
  ADD COLUMN kind         text NOT NULL DEFAULT 'http' CHECK (kind IN ('http', 'email', 'dns', 'schedule')),
  ADD COLUMN meta         jsonb,
  ADD COLUMN note         text CHECK (char_length(note) <= 2000),
  ADD COLUMN starred      boolean NOT NULL DEFAULT false,
  ADD COLUMN verification text CHECK (verification IN ('valid', 'invalid', 'missing')),
  ADD COLUMN rule_state   text CHECK (rule_state IN ('running', 'succeeded', 'failed')),
  ADD COLUMN imported     boolean NOT NULL DEFAULT false;

CREATE TABLE folders (
  id         uuid PRIMARY KEY,
  user_id    uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  name       text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
  position   integer NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (user_id, name)
);

CREATE TABLE folder_endpoints (
  folder_id   uuid NOT NULL REFERENCES folders ON DELETE CASCADE,
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  added_at    timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (folder_id, endpoint_id)
);
CREATE INDEX folder_endpoints_endpoint ON folder_endpoints (endpoint_id);

CREATE TABLE endpoint_aliases (
  alias       text PRIMARY KEY CHECK (alias ~ '^[a-z][a-z0-9-]{1,21}[a-z0-9]$'),
  endpoint_id text REFERENCES endpoints ON DELETE SET NULL,
  owner_id    uuid REFERENCES users ON DELETE SET NULL,
  created_at  timestamptz NOT NULL DEFAULT now(),
  released_at timestamptz
);
CREATE INDEX endpoint_aliases_endpoint ON endpoint_aliases (endpoint_id) WHERE endpoint_id IS NOT NULL;

CREATE TABLE endpoint_grants (
  token_hash  bytea PRIMARY KEY,
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  created_at  timestamptz NOT NULL DEFAULT now(),
  expires_at  timestamptz NOT NULL
);
CREATE INDEX endpoint_grants_endpoint ON endpoint_grants (endpoint_id);
CREATE INDEX endpoint_grants_expires ON endpoint_grants (expires_at);

CREATE TABLE endpoint_templates (
  id         uuid PRIMARY KEY,
  user_id    uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  name       text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
  config     jsonb NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (user_id, name)
);

CREATE TABLE endpoint_secrets (
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  name        text NOT NULL CHECK (name ~ '^[A-Z][A-Z0-9_]{0,63}$'),
  key_id      smallint NOT NULL,
  nonce       bytea NOT NULL CHECK (octet_length(nonce) = 24),
  ciphertext  bytea NOT NULL CHECK (octet_length(ciphertext) <= 4112),
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (endpoint_id, name)
);

CREATE TABLE endpoint_vars (
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  name        text NOT NULL CHECK (name ~ '^[a-z_][a-z0-9_]{0,63}$'),
  value       jsonb NOT NULL,
  updated_at  timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (endpoint_id, name)
);

CREATE TABLE rules (
  id          uuid PRIMARY KEY,
  endpoint_id text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  position    smallint NOT NULL,
  name        text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
  enabled     boolean NOT NULL DEFAULT true,
  condition   jsonb NOT NULL DEFAULT '{}',
  steps       jsonb NOT NULL,
  created_at  timestamptz NOT NULL DEFAULT now(),
  updated_at  timestamptz NOT NULL DEFAULT now(),
  UNIQUE (endpoint_id, position) DEFERRABLE INITIALLY DEFERRED
);

CREATE TABLE rule_runs (
  id              uuid PRIMARY KEY,
  capture_id      uuid NOT NULL REFERENCES captures (id) ON DELETE CASCADE,
  endpoint_id     text NOT NULL,
  trigger         text NOT NULL CHECK (trigger IN ('capture', 'rerun')),
  rules_version   integer NOT NULL,
  state           text NOT NULL CHECK (state IN ('sync', 'queued', 'running', 'succeeded', 'failed')),
  attempts        smallint NOT NULL DEFAULT 0,
  next_attempt_at timestamptz NOT NULL DEFAULT now(),
  locked_until    timestamptz,
  plan            jsonb,
  next_step       smallint NOT NULL DEFAULT 0,
  vars            jsonb NOT NULL DEFAULT '{}',
  log             jsonb NOT NULL DEFAULT '[]',
  response_status smallint,
  error           text,
  created_at      timestamptz NOT NULL DEFAULT now(),
  finished_at     timestamptz
);
CREATE INDEX rule_runs_due ON rule_runs (next_attempt_at) WHERE state IN ('sync', 'queued', 'running');
CREATE INDEX rule_runs_capture ON rule_runs (capture_id);

CREATE TABLE api_tokens (
  id           uuid PRIMARY KEY,
  user_id      uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  name         text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
  prefix       text NOT NULL,
  token_hash   bytea NOT NULL UNIQUE,
  scopes       text[] NOT NULL,
  endpoint_ids text[],
  created_at   timestamptz NOT NULL DEFAULT now(),
  expires_at   timestamptz NOT NULL,
  last_used_at timestamptz,
  revoked_at   timestamptz
);
CREATE INDEX api_tokens_user ON api_tokens (user_id);

CREATE TABLE notification_channels (
  id                 uuid PRIMARY KEY,
  endpoint_id        text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  created_by         uuid REFERENCES users ON DELETE SET NULL,
  kind               text NOT NULL CHECK (kind IN ('webhook', 'slack', 'discord', 'telegram', 'email', 'push')),
  name               text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 64),
  enabled            boolean NOT NULL DEFAULT true,
  trigger            text NOT NULL CHECK (trigger IN ('capture', 'rule')),
  condition          jsonb,
  config             jsonb NOT NULL,
  template           text,
  per_hour           integer NOT NULL CHECK (per_hour BETWEEN 1 AND 3600),
  disable_token_hash bytea,
  created_at         timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX notification_channels_endpoint ON notification_channels (endpoint_id);

CREATE TABLE notification_deliveries (
  id              uuid PRIMARY KEY,
  channel_id      uuid NOT NULL REFERENCES notification_channels ON DELETE CASCADE,
  capture_id      uuid REFERENCES captures (id) ON DELETE CASCADE,
  rule_run_id     uuid REFERENCES rule_runs ON DELETE CASCADE,
  kind            text NOT NULL CHECK (kind IN ('single', 'summary', 'test')),
  message         text,
  state           text NOT NULL CHECK (state IN ('pending', 'running', 'succeeded', 'failed', 'suppressed')),
  attempts        smallint NOT NULL DEFAULT 0,
  next_attempt_at timestamptz NOT NULL DEFAULT now(),
  locked_until    timestamptz,
  status          smallint,
  error           text,
  created_at      timestamptz NOT NULL DEFAULT now(),
  finished_at     timestamptz
);
CREATE INDEX notification_deliveries_due ON notification_deliveries (next_attempt_at) WHERE state IN ('pending', 'running');
CREATE INDEX notification_deliveries_capture ON notification_deliveries (capture_id);
CREATE INDEX notification_deliveries_channel ON notification_deliveries (channel_id, created_at);

CREATE TABLE email_addresses (
  id                uuid PRIMARY KEY,
  user_id           uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  address           text NOT NULL CHECK (char_length(address) <= 254),
  verified_at       timestamptz,
  verify_token_hash bytea,
  verify_expires_at timestamptz,
  created_at        timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX email_addresses_user_address ON email_addresses (user_id, lower(address));

CREATE TABLE push_subscriptions (
  id              uuid PRIMARY KEY,
  user_id         uuid NOT NULL REFERENCES users ON DELETE CASCADE,
  endpoint_url    text NOT NULL UNIQUE,
  p256dh          bytea NOT NULL,
  auth            bytea NOT NULL,
  user_agent      text,
  created_at      timestamptz NOT NULL DEFAULT now(),
  last_success_at timestamptz,
  failures        smallint NOT NULL DEFAULT 0
);
CREATE INDEX push_subscriptions_user ON push_subscriptions (user_id);

CREATE TABLE schedules (
  id                   uuid PRIMARY KEY,
  endpoint_id          text NOT NULL REFERENCES endpoints ON DELETE CASCADE,
  created_by           uuid REFERENCES users ON DELETE SET NULL,
  name                 text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 100),
  cron                 text NOT NULL,
  timezone             text NOT NULL DEFAULT 'UTC',
  request              jsonb NOT NULL,
  enabled              boolean NOT NULL DEFAULT true,
  paused_reason        text,
  next_run_at          timestamptz,
  last_run_at          timestamptz,
  consecutive_failures smallint NOT NULL DEFAULT 0,
  locked_until         timestamptz,
  created_at           timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX schedules_due ON schedules (next_run_at) WHERE enabled;
CREATE INDEX schedules_endpoint ON schedules (endpoint_id);

CREATE TABLE sent_requests (
  id                uuid PRIMARY KEY,
  user_id           uuid REFERENCES users ON DELETE CASCADE,
  endpoint_id       text REFERENCES endpoints ON DELETE CASCADE,
  source_capture_id uuid,
  created_at        timestamptz NOT NULL DEFAULT now(),
  request           jsonb NOT NULL,
  result            jsonb NOT NULL,
  response_body     bytea CHECK (octet_length(response_body) <= 65536),
  CHECK (user_id IS NOT NULL OR endpoint_id IS NOT NULL)
);
CREATE INDEX sent_requests_user ON sent_requests (user_id, created_at DESC) WHERE user_id IS NOT NULL;
CREATE INDEX sent_requests_endpoint ON sent_requests (endpoint_id, created_at DESC) WHERE endpoint_id IS NOT NULL;

CREATE TABLE usage_counters (
  subject      text NOT NULL,
  metric       text NOT NULL,
  window_start timestamptz NOT NULL,
  count        bigint NOT NULL,
  PRIMARY KEY (subject, metric, window_start)
);
CREATE INDEX usage_counters_window ON usage_counters (window_start);
```

| Rule | Detail |
|---|---|
| Migrations | One migration per milestone that needs it; `ADD COLUMN ... DEFAULT` with a constant is metadata-only on Postgres 16. |
| Capture insert (§2.1) | The `UPDATE endpoints` condition becomes `(never_expires OR last_active_at > now() - make_interval(secs => COALESCE(retention_seconds, $default)))` and `capture_count < COALESCE(max_captures, $default)`; `RETURNING` adds `rules_version`, `rule_count`, `rules_sync`, `capture_channel_count`, `response_templated`. Ring mode deletes the lowest `seq` with `NOT starred`; when every capture is starred the endpoint refuses as full. |
| Early check (§5) | The existence `SELECT` also returns `sender_allow_cidrs`, `capture_auth`, `verifier` (with its sealed secret joined), `email_enabled`, `dns_enabled`, so access checks run before the body is read. |
| Endpoint flags | `rules_version`, `rule_count`, `rules_sync` (any rule has a response step) and `capture_channel_count` are rewritten in the same transaction as every rule or channel change. |
| Size caps in code | `rule_runs.vars` 16 KiB, `rule_runs.log` 64 KiB, `rule_runs.plan` 128 KiB, `endpoint_vars.value` 4 KiB, total rules of one endpoint 128 KiB serialized. |
| Retention of side tables | `sent_requests`: newest 50 per user or endpoint, older deleted on insert. `notification_deliveries` and `rule_runs` follow their capture. Grants, login states, expired tokens and old usage windows are purged by the maintenance job. |

## 21. Template language

**Decision: a purpose-built substitution language in `whl-template`: `{{ expression }}` with pipes, no loops,
no conditionals, no assignment.** Output is linear in input, so a size cap and a deadline are sufficient
limits; values are never re-parsed, so a captured `{{` cannot inject code; branching belongs to rules (§22) and
scripts (§23), which already have their own limits. Rejected: MiniJinja (loops and macros multiply output and
need fuel plus writer caps, a wider surface for a feature that only substitutes); Handlebars (helpers and
partials); Tera (no execution limits).

```
template = { text | "{{" expr "}}" }
expr     = primary { "|" filter }
primary  = path | call | string | integer
path     = ident { "." key | "[" ( integer | string ) "]" }
call     = ident "(" [ arg { "," arg } ] ")"
filter   = ident [ "(" arg { "," arg } ")" ]
arg      = path | string | integer
```

`{{ "{{" }}` writes literal braces. Whitespace inside `{{ }}` is ignored; outside, text is copied verbatim.

| Root | Values |
|---|---|
| `request` | `method`, `path`, `segments[n]`, `query.<name>`, `header.<name>` (case-insensitive, first value), `headers` (all as JSON), `form.<name>`, `body` (UTF-8 lossy, first 64 KiB), `ip`, `content_type`, `kind`, `meta.<key>`, `signature` (`valid`, `invalid`, `missing`) |
| `capture` | `id`, `seq`, `received_at` |
| `endpoint` | `id`, `url`, `title` |
| `var`, `store`, `counter` | Run variables, persistent variables, persistent counters |
| `step.<name>` | `status`, `header.<name>`, `body`, `duration_ms`, `error` of an earlier HTTP or script step |
| `secret.<NAME>` | Decrypted secret; outbound fields only (§18) |
| `now` | Capture time (rules) or render time (schedules, notifications) |

| Functions | `json("<RFC 9535 path>")` on the request body (first match, compact JSON for arrays and objects), `json(step.<name>.body, "<path>")`, `uuid()`, `random_hex(n)` (n ≤ 64), `random_int(a, b)` |
|---|---|
| Filters | `default(x)`, `upper`, `lower`, `trim`, `truncate(n)`, `replace(a, b)` (literal), `length`, `json` (JSON string literal), `url`, `html`, `base64`, `base64_decode`, `hex`, `sha256`, `hmac_sha256(key)`, `date(format)` |

| Aspect | Behaviour |
|---|---|
| Parsing | Hand-written recursive descent, errors with line and column; compiled once per (endpoint, version) and cached per replica. |
| JSON body | Parsed lazily once per run with `serde_json`, only when the body is at most 1 MiB, nesting at most 64; JSON path by `serde_json_path` (RFC 9535). |
| Escaping | None implicit. Responses go to the sender under `Content-Security-Policy: sandbox`, never to the viewer; the author picks `json`, `html` or `url` where the target format needs it. |
| Limits | Source 64 KiB; 500 expressions; output written through a bounded writer (body 1 MiB, header value 1024 bytes, status 3 digits); deadline checked before each expression (50 ms per render). |
| Determinism | `uuid`, `random_*` draw from a ChaCha generator seeded by the run id, so a re-run renders the same values. |
| Rendered headers | Validated with the §9 rules after rendering; a failing header is dropped and logged, the rest of the response stands. |

## 22. Rules engine and execution model

**Decision: a rule list per endpoint, executed as one ordered plan that is cut into a before-response part and
an after-response part at a statically known point: the last response step.** Everything before the cut runs
on the capture path under a time budget; side effects found before the cut are deferred with a snapshot of the
variables; everything after the cut runs in a durable worker. Endpoints whose rules contain no response step
add no latency at all. Rejected: running all steps after the response (cannot shape the response, the main
parity need); running all steps before it (notifications and retries on the sender's clock, nothing durable);
an external workflow engine (a new stateful service).

| Step | Class | Before the cut | After the cut |
|---|---|---|---|
| `respond`, `set_header`, `remove_header`, `delay`, `stop` | response | Runs | No-op, logged (the response is gone) |
| `extract`, `set_var`, `transform`, `script` | compute | Runs | Runs |
| `set_store`, `increment` | state | Runs, one statement each, 200 ms | Runs |
| `http` | fetch | `member` tier, at most 2, inside the budget | Runs, retried |
| `forward`, `notify`, `store_s3` | side effect | Deferred with a variable snapshot | Runs |

| Aspect | Behaviour |
|---|---|
| Cut point | Rules are evaluated in order, each condition when the rule is reached. The cut is after the last response step of the last rule that has one, known at save time (`rules_sync`). Rules after the cut evaluate their conditions in the worker. |
| Capture transaction | When `rule_count > 0`, insert `rule_runs` with `state = 'sync'`, `locked_until = now() + 30 s` if `rules_sync`, else `state = 'queued'`; `pg_notify('whl_rules_wake')` when queued. |
| Rule set load | Per-replica cache keyed by `(endpoint_id, rules_version)` from the `RETURNING`; a miss costs one `SELECT` with a 100 ms statement timeout. |
| Before-response part | Runs after commit and before the delay. Budget `WEBHOOKS_LIVE_RULES_SYNC_BUDGET` (3 s, at most 10 s); each HTTP step gets `min(5 s, remaining)`; each script 100 ms (`member`). Concurrency per replica `WEBHOOKS_LIVE_RULES_SYNC_CONCURRENCY` (256). |
| Post-sync write | One transaction: `rule_runs` gets `state` (`queued`, `succeeded` or `failed`), `plan` (remaining rules plus deferred side effects with snapshots), `vars`, `log`, `next_step`, `response_status`; `captures` gets `response_status` and `rule_state`; then `pg_notify` and a `rule_result` event. Statement timeout 2 s. |
| Post-sync write fails | The computed response is still sent. The `sync` lease expires and the worker replays the run from the start with response steps as no-ops. |
| Worker | Claims `queued` runs and expired leases like §6, 16 runs in flight per replica, HTTP steps share the forward host semaphores. After every step it persists `next_step`, `vars` and the log entry, so a crash resumes at the next step. |
| Side-effect idempotency | Forward jobs and notification deliveries created by a step get id `UUIDv5(run id, step index)` and are inserted `ON CONFLICT DO NOTHING`: exactly one enqueue per step. HTTP steps send `X-Webhooks-Live-Delivery: <run id>:<step index>` so targets can deduplicate at-least-once retries. |
| Retries | HTTP steps: the forward policy (3 attempts, 10 s and 60 s). A step that finally fails stops the run unless `continue_on_error`. A non-2xx answer is data (`step.<name>.status`), not a failure; transport errors and SSRF refusals are failures. |
| Hop guard | Every outbound request from rules, forwards, schedules and the composer carries `X-Webhooks-Live-Hop: n+1`, where `n` is the hop count of the capture that caused it. Rules and forward rules do not run on captures with hop 3 or more (`rule_state` stays null, logged). Stops a rule calling its own endpoint through any alias or subdomain. |
| Re-run | Creates a `rerun` run, `queued`, using the current rules with response steps as no-ops. |
| Dry run | Runs the whole plan in the API request against a stored capture, at most 10 s; side effects and state writes become "would do" log entries; HTTP steps run only with `execute_http: true`, quota counted. Not stored. |

| Before-response failure | Sender gets | Run |
|---|---|---|
| Template or render error | Static response, `X-Webhooks-Live-Rule-Error: template` | `failed` unless the step is `continue_on_error` |
| Budget overrun | Static response, `budget` | `failed`; deferred side effects dropped |
| Script limit or error | Static response, `script` | `failed` unless `continue_on_error` |
| HTTP step transport error | Depends on later steps (`step.x.error`) | Fails unless `continue_on_error` |
| Quota exceeded | Static response, `quota` | `failed`, error `quota_exceeded` |
| Sync concurrency full | Static response, `overloaded` | Queued; the after-cut part still runs |
| Internal error | Static response, `internal` | `failed` |

Run log entry: `{rule_id, rule_name, matched}` per evaluated rule and `{rule_id, step, name, kind, outcome,
duration_ms, detail, output}` per step, where `outcome` is `ok`, `failed`, `skipped`, `deferred` or `no_op`,
`detail` at most 1 KiB and `output` at most 4 KiB, redacted (§18).

Validation lives in `whl-rules` (`serde` with unknown fields denied). Step names are unique within an endpoint
when they produce outputs. Regexes use the `regex` crate (linear time, no backreferences) with
`size_limit` 1 MiB; JSON paths are parsed on save.

## 23. Script sandbox

**Decision: embed Rhai (`rhai`, MIT or Apache-2.0) in `whl-script`, with no I/O registered, every built-in limit
set, wall time and memory enforced from the progress callback, and execution on a dedicated small thread
pool.** Rhai is pure Rust, sandboxed by default (a script reaches only what the host registers), exposes limits
for operations, call depth, expression depth and container sizes, and has a syntax close to JavaScript and
Rust. Rejected: **WASM** (wasmtime) gives the strongest isolation and exact memory limits but needs users to
compile code or an embedded JavaScript-in-WASM interpreter, adds a JIT and about 15 MiB to the binary, and costs
the most to build; **a purpose-built language** (we would own an interpreter's bugs and its documentation);
**Lua through `mlua`** (C code, a history of sandbox escapes through the standard library); **JavaScript
through `boa`** (large, weak limit controls).

| Control | Setting |
|---|---|
| Engine | `Engine::new_raw()` plus `CorePackage`, `LogicPackage`, `BasicStringPackage`, `BasicArrayPackage`, `BasicMapPackage`, `BasicMathPackage`. No time package, no module resolver (`DummyModuleResolver`), `eval` and `import` disabled with `disable_symbol`. Crate features: `sync`, `no_custom_syntax`; `unchecked` and `internals` are never enabled (a test asserts the limits still fire). |
| Host functions | `json_parse`, `json_stringify`, `base64_encode`, `base64_decode`, `hex`, `sha256`, `hmac_sha256`, `regex_match`, `regex_captures` (the `regex` crate, size-limited), `uuid`, `random_int`, `now` (capture time). Nothing touches the network, files, environment or wall clock. |
| Inputs and outputs | `request` (read-only map built from §21 roots), `vars` (mutable copy of run variables); the script returns a map with optional `response` (`status`, `headers`, `body`, `content_type`) and `vars`. Only the returned map leaves the sandbox. |
| Limits | Operations 100 000 before the response, 1 000 000 after; call levels 32; expression depth 64 (32 inside functions); string 16 KiB; array 256; map 256; functions 32; source 16 KiB. |
| Wall time | `on_progress` checks a deadline every 1024 operations: tier value (§17); queue wait counts. |
| Memory | The global allocator wraps `mimalloc` with a counter that is active only on script threads (one thread-local add per allocation); `on_progress` aborts above 32 MiB. Overshoot between checks is bounded by the container and string limits. |
| Threads | `WEBHOOKS_LIVE_SCRIPT_THREADS` (2) dedicated threads with 8 MiB stacks and a bounded queue; a panic is caught (`catch_unwind`) and reported as `script_panic`. |
| Output | `print` and `debug` go to the run log, 4 KiB total. |
| Compile | On save, with the same limits; AST cached per replica by `(rule id, step index, rules_version)`, at most 1000 entries. |
| Supply chain | Rhai version pinned; updates reviewed as security changes; a fuzz target over the engine configuration in `tools/fuzz`. |

## 24. Endpoint management additions

| Feature | Design |
|---|---|
| Retention | §20 capture-insert condition. Maintenance soft-deletes by the same rule. Storage-cap eviction order: anonymous endpoints by least recent activity, then owned ones; never-expire endpoints are never evicted (bounded by owner storage quotas). |
| Capture TTL | Maintenance deletes `captures` with `received_at < now() - capture_ttl_seconds` in batches of 500 per endpoint, correcting counters in the same transaction. |
| Per-endpoint cap | Set at creation from the tier, editable by the owner up to the tier maximum. |
| Tags, folders | Tags on the endpoint (shared, editors); folders per user (private). Endpoint list filters `tag` and `folder`. |
| Aliases | Routes `/a/{alias}` and `/a/{alias}/{*rest}` resolve the alias (per-replica cache: positive 30 s, negative 5 s) and enter the capture pipeline with `rest` as sub-path; `<alias>@<mail domain>` delivers too (§28). The `/a/` prefix keeps aliases out of the root namespace, so future UI routes cannot collide. Reserved: `a`, `admin`, `api`, `assets`, `auth`, `dns`, `docs`, `e`, `endpoints`, `help`, `login`, `logout`, `mail`, `new`, `settings`, `signin`, `static`, `status`, `www`. Delete or endpoint delete sets `released_at`; another owner may take it after `WEBHOOKS_LIVE_ALIAS_QUARANTINE` (30 d), the same owner at once. |
| Sender allow list | Checked after id resolution, before reading the body: client IP outside every range answers 403, `X-Webhooks-Live-Error: sender_refused`, `Connection: close` if the body is unread. Applies to SMTP clients too. |
| Capture auth | Basic: username compared exactly, password as MAC (§18); header: name case-insensitive, value as MAC; both constant-time. Failure: 401, `X-Webhooks-Live-Error: unauthorized`, no `WWW-Authenticate` (§9 denylist reasoning). CORS preflights (which cannot carry credentials) are answered but not stored. |
| View password | Anonymous endpoints only (422 `not_allowed` on owned ones). `POST .../unlock` limited to 5/min per endpoint and 20/min per IP; returns a 32-byte grant, stored as SHA-256 in `endpoint_grants`, valid 12 h. Accepted as `X-Webhooks-Live-Grant` and, on stream routes only, `?grant=`; request logs never record query strings. Changing or removing the password deletes all grants. |
| Clone, templates | A configuration document `{"version": 1, "endpoint": EndpointInput, "forward_rules", "rules", "channels", "schedules"}` without secrets; clone also copies secrets when the caller owns both endpoints. Templates store the same document; creating from a template re-validates against current limits. |
| Signature verifier | Computed after the body is read and before the store transaction; result in `captures.verification`. Schemes: `hmac` (header, algorithm `sha1`, `sha256` or `sha512`, encoding `hex` or `base64`, optional prefix, signed content `body` or `timestamp.body` with a timestamp header and a 300 s tolerance) and presets that fill those fields for common providers (`github`, `stripe`, `slack`, `shopify`, `standard_webhooks`). Constant-time comparison. |
| Subdomain capture | `Host` matching `^[0-9a-hjkmnp-tv-z]{24}\.<WEBHOOKS_LIVE_SUBDOMAIN_CAPTURE_HOST>$` (case-insensitive) enters the capture pipeline with the full path as sub-path. Needs a wildcard DNS record, a wildcard certificate and a listener hostname. The `__Host-` session cookie is never sent to subdomains. |
| Stats | `GROUP BY` over the endpoint's retained captures (at most the cap), bucketed by hour or day. |

## 25. Notifications

**Decision: a durable delivery queue (`notification_deliveries`, §6 pattern) fed by the capture transaction and
by rule steps, with adapters that pin chat vendors to fixed hosts, email only to verified addresses, and quotas
per channel and owner.** Rejected: sending from the capture path (latency, no retry); a third-party
notification service (a new dependency holding user data).

| Channel | Target and format | Guard |
|---|---|---|
| `webhook` | `POST` JSON `{endpoint_id, capture: CaptureSummary, message}` to any URL | SSRF guard of §6; save-time target check |
| `slack` | `POST {"text"}` to the stored incoming-webhook URL | URL must match `^https://hooks\.slack\.com/services/[A-Za-z0-9/_-]+$`; `&`, `<`, `>` escaped so no mention or link markup is interpreted |
| `discord` | `POST {"content", "allowed_mentions": {"parse": []}}` | URL must match `^https://(discord\|discordapp)\.com/api/webhooks/[0-9]+/[A-Za-z0-9_-]+$`; content at most 2000 characters |
| `telegram` | `POST https://api.telegram.org/bot<token>/sendMessage` with `chat_id`, `text`, `disable_web_page_preview` | Token is a secret (§18); no `parse_mode`, so no markup; `429` honours `retry_after` |
| `email` | SMTP submission to `WEBHOOKS_LIVE_SMTP_URL`, plain text, `From: WEBHOOKS_LIVE_NOTIFY_FROM` | Recipient must be a verified address of the channel's creator; `List-Unsubscribe` with `List-Unsubscribe-Post: List-Unsubscribe=One-Click`; 50/day per user |
| `push` | Web Push (RFC 8030, 8291, 8292) to the creator's subscriptions, payload `{endpoint_id, capture_id, kind, method, path}` at most 3 KiB | Subscription host must be in a vendor list (`fcm.googleapis.com`, `updates.push.services.mozilla.com`, `*.push.apple.com`, `*.notify.windows.com`); 404 or 410 deletes the subscription |

| Aspect | Behaviour |
|---|---|
| Triggers | `capture`: deliveries inserted in the capture transaction for each enabled capture-trigger channel whose condition (§22 condition model, evaluated before the transaction) matches; `capture_channel_count` skips the lookup when zero. `rule`: notify steps (§22). Hop 3 or more never notifies. |
| Message | §21 template; default names endpoint, kind, method, path and a link `/e/<id>/c/<capture id>`. Rendered, then truncated to the channel limit (Discord 2000, Telegram 4096, Slack 40000, email 64 KiB). |
| Rate limits | Before sending: `usage_counters` for `c:<channel>` (`per_hour`) and the owner subject (§17). Over the limit the delivery becomes `suppressed`; when the hour closes, the maintenance job enqueues one `summary` delivery per channel with the suppressed count. |
| Retries | 3 attempts, 10 s and 60 s; result shown on the capture (`notifications` in Capture). |
| Vendor egress | Slack, Discord, Telegram and push hosts are constants, so their requests may use `WEBHOOKS_LIVE_VENDOR_PROXY` (HTTP CONNECT) when set; prod1's Moscow egress does not reliably reach Discord and Telegram. The generic webhook never uses the proxy (§6 guard). |
| Email addresses | Verification link with a 32-byte token (SHA-256 stored, 24 h), at most 5 addresses and 5 verification mails a day per user. The IdP-verified sign-in address (`email_verified` claim) counts as verified. |
| Web Push keys | VAPID P-256 key from `WEBHOOKS_LIVE_VAPID_PRIVATE_KEY`; message encryption with the `web-push` crate's builder, sent through the shared client. |

## 26. Outbound requests: composer, schedules, storage

**Decision: one `OutboundClient` in `whl-forward` for every purpose (forward, rule, schedule, composer,
webhook channel, S3), with the §6 resolver, ranges and redirect rules, a purpose label, and a quota check.**
One guard to audit. Rejected: per-feature clients (each a new SSRF surface).

| Aspect | Composer | Schedules |
|---|---|---|
| Entry | `POST /api/v1/send`, synchronous, timeout at most 15 s | Claim loop: `UPDATE schedules SET locked_until = now() + 60 s WHERE id IN (SELECT id FROM schedules WHERE enabled AND next_run_at <= now() AND (locked_until IS NULL OR locked_until < now()) ORDER BY next_run_at LIMIT 8 FOR UPDATE SKIP LOCKED) RETURNING *` |
| Request | Method, URL, headers (§9 hop-by-hop rules), body up to 1 MiB (route body limit raised to 1.5 MiB) | Request template (§21), secrets allowed in URL and headers, body template at most 64 KiB |
| Result | JSON: status, headers, body (first 1 MiB, Body object), duration, final URL, error. Never re-served as a real response. | A `schedule` capture through the normal store path (limits, rules after the cut, notifications); endpoint full or gone counts as a failure |
| Self target | URL host equals the public host, an alias path or a subdomain capture: handled in process by the capture pipeline with the caller's IP and `X-Webhooks-Live-Composer: 1`, no network hop | Same short-circuit |
| History and state | `sent_requests`, newest 50 per user (or endpoint for anonymous callers) | `next_run_at` from `croner` with `chrono-tz` from now (no backfill); `consecutive_failures` 10 pauses with `paused_reason` |
| Save checks | – | Cron parsed; the next 10 occurrences must be at least the tier interval apart; time zone must be an IANA name |
| Quota | `outbound` per subject | `outbound` per owner |

S3 storage step: `PUT` of the capture as JSON, or of the raw body, to `<bucket URL>/<prefix><endpoint id>/<seq>-<capture id>.<ext>`
with AWS Signature V4 (`aws-sigv4`), credentials as two named secrets, bucket URL through the guard.

## 27. Export and import

**Decision: exports stream from a producer task through a bounded channel into the response body; one capture
body is in memory at a time.** Rejected: building files in memory or on disk (unbounded memory; distroless pods
have no scratch volume) and background export jobs with download links (a second storage of the same data).

| Aspect | Behaviour |
|---|---|
| Producer | Reads 50 captures per page by `seq`, then each body separately; channel capacity 4 chunks of 64 KiB. |
| Concurrency | 2 per replica (`WEBHOOKS_LIVE_EXPORT_CONCURRENCY`), 1 per caller; over: 429. |
| Size check | Before streaming, `sum(stored_bytes)` of the selection against the tier export cap: 422 `export_too_large`. |
| `json` | `{"format": "webhooks-live-export", "version": 1, "exported_at", "endpoint": {...}, "captures": [Capture with the whole body]}` |
| `ndjson` | First line the header object without `captures`, then one Capture per line. |
| `csv` | `id, seq, kind, received_at, method, path, query_string, client_ip, content_type, body_size, response_status, headers_json, body_preview` (first 4 KiB as text, or `base64:` prefix). Cells starting with `=`, `+`, `-`, `@`, tab or CR get a leading `'`. |
| `zip` | `captures.ndjson` (Captures with `body` null) and `bodies/<seq>-<capture id>.bin`, written by `async_zip` with data descriptors and ZIP64; index deflated, bodies stored. Entry names come only from `seq` and ids. |
| Excluded | Secrets, capture-auth MACs, view password, grants. |
| Gateway | The export route needs `timeouts.request: 0s` like the stream (§12 route change). |
| Import | `POST .../import`, body JSON or NDJSON, streamed through `serde_json` with a sequence visitor (one capture in memory), size cap by tier, nesting limit 64. Each capture is validated with the capture-path limits, inserted in batches of 50 with new ids and `seq`, original `received_at`, `imported = true`, `meta.original_id`. Stops at capacity; answers counts. Emits `captures_imported`; no rules, forwards or notifications. |

## 28. Email capture

**Decision: an inbound-only SMTP receiver in the workspace (`whl-mail`, in the same binary on port 2525),
reached from the internet through the Moscow entry and the gateway's TCP listener with PROXY protocol v2.** It
needs nothing from the company mail system, refuses unknown recipients during the dialogue (no backscatter),
stores durably before answering 250, and preserves the client address. Rejected: routing the domain through
the company Stalwart server by LMTP (its spam filter would drop test mail, and abuse against the catcher would
load the company mail server); a webhook from an MTA (still needs an MTA in front).

| Aspect | Behaviour |
|---|---|
| Libraries | `smtp-proto` (command parsing), `mail-parser` (MIME), `tokio-rustls` for STARTTLS; all MIT or Apache-2.0. |
| Listener | `WEBHOOKS_LIVE_MAIL_LISTEN_ADDR` (unset: off). A PROXY v2 header is required from peers in `WEBHOOKS_LIVE_MAIL_TRUSTED_PROXIES`; connections from other peers are refused unless the list is empty (local development). |
| Commands | `EHLO`, `HELO`, `MAIL`, `RCPT`, `DATA`, `RSET`, `NOOP`, `QUIT`, `STARTTLS`. Extensions: `SIZE`, `8BITMIME`, `SMTPUTF8`, `PIPELINING`, `ENHANCEDSTATUSCODES`, `STARTTLS` (only with a certificate). No `AUTH`, `VRFY`, `EXPN`, `BDAT`, no relaying. |
| Recipients | Domain must equal `WEBHOOKS_LIVE_MAIL_DOMAIN` (case-insensitive). Local part: a 24-character id (case-insensitive), `id+tag`, or an alias. Endpoint must be live and `email_enabled`; the sender allow list applies to the client IP. Else `550 5.1.1`. At most 10 recipients per message; one capture per distinct endpoint. |
| Size and time | `SIZE` advertises the tier limit (10 MiB); over it during `DATA`: `552 5.3.4` and close. Command timeout 60 s, `DATA` 120 s, session 10 min, 5 messages per session. 200 sessions per replica, 5 per client IP, 30 messages per minute per client IP. |
| Quota | `usage_counters` `e:<id>` `email_in` per hour; over: `451 4.7.1`. |
| Store | Parse in `spawn_blocking` with a 2 s timeout and at most 50 MIME parts; build the §20 mapping; store through the normal capture path; `250 2.0.0 <capture id>` after commit. Store down: `451 4.3.0`. Endpoint full: `552 5.2.2`. |
| Parts | `GET .../parts/{index}` decodes one part from the raw message on demand; served with the raw-body headers of api §5.2; never `text/html`. |
| TLS | Certificate and key from `WEBHOOKS_LIVE_MAIL_TLS_CERT` and `_KEY` files (a cert-manager secret), reloaded every 60 s. |

| Exposure option | Path | Pros | Cons |
|---|---|---|---|
| **A (recommended)** | MX `mx.<domain>` to the Moscow entry; rlb1 haproxy TCP 25 with `send-proxy-v2` to the Moscow gateway; Envoy Gateway TCP listener 25 with `ClientTrafficPolicy` PROXY protocol; `TCPRoute` to Service `webhooks-live-mail:2525`; `BackendTrafficPolicy` `proxyProtocol: V2` to the pod | Independent of company mail; one hop; real client address; real-time failures visible to the sender | Inbound 25 must be open at the Moscow provider and on rlb1; Envoy Gateway must accept a `BackendTrafficPolicy` on a `TCPRoute` (supported in practice, check the installed version) |
| B (fallback) | MX to ams1; ams1 postfix adds the domain to `relay_domains` with a transport to the cluster entry on 2525 | ams1 is already public on 25 with TLS | Couples the company relay to catcher abuse; client address replaced by ams1 (kept only in `Received`); ams1 queues hide failures |
| C | ams1 to Stalwart, Stalwart routes the domain by LMTP | – | As rejected above |

DNS for option A: `<mail domain> MX 10 mx.webhooks.live.`, `mx.webhooks.live A 89.235.182.84`,
`<mail domain> TXT "v=spf1 -all"`, `_dmarc.<mail domain> TXT "v=DMARC1; p=reject"` (the domain never sends).
A subdomain such as `email.webhooks.live` keeps the apex free for real company mail.

## 29. DNS capture

**Decision: an authoritative responder written in `whl-dns` on `hickory-proto` (wire format only), UDP and TCP
on port 5353 in the same binary, fed by dnsdist at the edge with PROXY protocol v2.** We answer one synthetic
zone, so a full server framework would be bypassed anyway; owning the loop gives exact control of rate limits,
answer size and capture. Rejected: `hickory-server` (its catalog and authority model target zone files and
has no hook for per-datagram PROXY headers); the PowerDNS remote backend (couples to the company DNS, and its
packet cache would hide repeated queries).

| Aspect | Behaviour |
|---|---|
| Parsing | Opcode other than QUERY: NOTIMP. Not exactly one question: FORMERR. Class other than IN: REFUSED. Name outside `WEBHOOKS_LIVE_DNS_ZONE`: REFUSED. Recursion never offered (RA = 0). |
| Apex | SOA (`WEBHOOKS_LIVE_DNS_NS_NAME`, `WEBHOOKS_LIVE_DNS_SOA_EMAIL`, serial from build time, negative TTL 60) and NS. |
| Captured names | The label immediately left of the zone, lowercased, must be a live endpoint id with `dns_enabled`. A: `WEBHOOKS_LIVE_DNS_ANSWER_A` or empty; AAAA likewise; TXT: the capture id; others: NOERROR, no data. TTL 0, so every resolution arrives. Unknown or disabled: NXDOMAIN, not stored. |
| Lookup | Per-replica cache of endpoint state, positive and negative 10 s, so floods of random names never reach Postgres. |
| PROXY v2 | Required from `WEBHOOKS_LIVE_DNS_TRUSTED_PROXIES` on UDP (header prefixed to each datagram) and TCP; the resolver address comes from it. |
| Amplification | UDP answers at most 512 bytes, or the EDNS size capped at 1232; `ANY` answered with the RFC 8482 HINFO record; no DNSSEC; response rate limiting per /24 or /56: 20 answers/s, above it every second answer is truncated (TC) and the rest dropped. |
| Capture rate | In-process limiter per endpoint plus `usage_counters` `dns_in` per hour (tier); over: answered, not stored. |
| Store | Normal capture path, empty body, §20 mapping. Store errors answer SERVFAIL. |

| Exposure option | Path | Pros | Cons |
|---|---|---|---|
| **B (recommended)** | A dedicated public IPv4 on the Moscow edge with dnsdist (`SuffixMatchNodeRule` on the zone to a pool of the cluster's UDP and TCP 5353 service with `useProxyProtocol=true`, `MaxQPSIPRule` as an outer limit); the parent zone delegates `NS` plus glue to that address | Isolated from the company DNS; abuse cannot touch other zones | Needs a spare public address and a dnsdist host |
| A (fallback) | dnsdist in front of PowerDNS on prod1-isp1 on the existing `ns1.globalart.dev` address: zone suffix to webhooks.live, everything else to PowerDNS moved to a local port | No new address or port exposure | Changes the company's authoritative DNS host, which ISPmanager also manages; a mistake affects every zone |
| C | Envoy Gateway `UDPRoute` | – | No PROXY protocol for UDP, so the resolver address is lost; port 53 on the Moscow entry already serves `ns1` |

Records in the `webhooks.live` zone: `<zone> NS ns-capture.webhooks.live.` and glue
`ns-capture.webhooks.live A <edge address>`; the zone name (for example `dns.webhooks.live`) is the owner's.
Fact to check before option A or B: how rlb1 forwards port 53 to prod1-isp1 today, and whether the source
address survives.

## 30. Combined inbox

| Aspect | Behaviour |
|---|---|
| Set | Signed in: all owned and shared endpoints, or one folder, at most 50; anonymous: `endpoints=` list of at most 20 ids, each checked like a single-endpoint read (grants included). |
| List | Per endpoint a keyset query `ORDER BY seq DESC LIMIT n` with the §5.1 filters; merged in the app by `(received_at, id)`. Cursor: opaque base64url of the per-endpoint `before_seq` map. |
| Stream | Subscribes to each endpoint's hub channel and merges them; event `id` is an opaque base64url vector of per-endpoint `seq`; resume replays `seq` above the vector per endpoint, at most 100 in total, else `resync`. Access is checked on connect; a revoked membership takes effect at the next reconnect (streams close after 30 min). |

## 31. Command-line forwarder

**Decision: `whl-cli`, binary `webhooks-live-cli`, built on `reqwest` with a small SSE parser of its own and a
per-endpoint state file.** It uses only the public API, so it works against any instance and version that
serves api.md. Rejected: WebSocket tunnels (a new server protocol); bundling the forwarder into the server
binary (users need a small download).

| Aspect | Behaviour |
|---|---|
| Commands | `forward <endpoint id or URL> --to <local URL>`, `login` (stores a token in the user config directory, mode 0600), `logout`. |
| Options | `--token` (or `WEBHOOKS_LIVE_TOKEN`), `--from now\|<seq>`, `--kinds http,email,...` (default `http`), `--strip-prefix`, `--keep-host`, `--concurrency` (default 1, in order), `--state-file`. |
| Loop | Open the stream with `Last-Event-ID` = last forwarded `seq`; on `ready` list `after_seq` ascending to the end; then live events. Each capture: fetch detail and raw body, send to the local target, persist `seq` (write and rename). `resync`: list again. `bye`, errors and EOF: reconnect with backoff 1 s doubling to 30 s with jitter. |
| Local request | Method; URL = target + sub-path (prefix stripped) + query; captured headers minus hop-by-hop, `Host`, `Content-Length`; body bytes; 30 s timeout; failures are printed, not fatal. |
| Delivery | At least once: a crash between sending and persisting repeats one capture. |
| Release | GitHub Actions matrix: Linux x86_64 and aarch64 (musl), macOS x86_64 and aarch64, Windows x86_64; SHA-256 checksums; `cargo install` from the repository. |

## 32. Security analysis

| Feature | Threat | Control |
|---|---|---|
| Templates | Template injection | Single pass; inserted values are data and never parsed (§21) |
| Templates | Output blow-up, slow render | Bounded writer, 500 expressions, 50 ms deadline, lazy JSON with size and depth caps |
| Templates | Header injection, XSS through the response | §9 header validation after render; capture responses keep `CSP: sandbox` and `nosniff` |
| Rules | SSRF through `http`, `forward`, `store_s3`, redirects, DNS rebinding | One `OutboundClient` with the §6 resolver and per-hop checks (§26) |
| Rules | Loops (rule calls its own endpoint via id, alias or subdomain) | Hop header; no rules or forwards at hop 3; outbound quotas bound external loops |
| Rules | Relay and flooding of third parties | Per-tier outbound quotas; anonymous tier has no before-response HTTP; identifying `User-Agent` |
| Rules | ReDoS | `regex` crate (linear time) with size limits |
| Scripts | Sandbox escape, CPU, memory, stack | §23: nothing registered beyond pure functions, operation, depth, size, time and memory limits, dedicated threads, pinned version, fuzzing |
| Secrets | Exfiltration to senders | Refused in response steps and templates; masked in logs; write-only API; editors are trusted by design |
| Secrets | Database leak | AEAD with keys outside the database; AAD binds endpoint and name |
| Notifications | Spam relay | Email only to verified addresses with one-click disable and daily caps; chat channels limited per hour with summaries |
| Notifications | SSRF | Webhook through the guard; chat and push hosts pinned by pattern |
| Notifications | Mass mentions, markup injection | Discord `allowed_mentions` empty; Slack escaping; Telegram without `parse_mode` |
| Email capture | Open relay, backscatter | No relaying, recipients refused in the dialogue |
| Email capture | Parser bombs, slow clients | Size cap, 50 parts, parse timeout, command and session timeouts, per-IP session caps |
| Email capture | Stored XSS, malware | HTML parts never served as HTML; parts as `application/octet-stream` or `text/plain` with `nosniff` and sandbox CSP |
| DNS capture | Reflection and amplification | Small answers, minimal `ANY`, no DNSSEC, response rate limiting, dnsdist limits at the edge |
| DNS capture | Database flood with random names | Negative cache; Postgres touched only for live enabled ids |
| PROXY protocol | Spoofed client addresses | Header accepted only from trusted proxy ranges; required from them |
| API tokens | Theft, over-privilege | Hashed, shown once, expiry required, scopes capped by role, revocation, no token management by token |
| API CORS | Cross-site use of the session | `*` without credentials; origin check stays for cookies |
| Aliases | Takeover after release, impersonation | 30-day quarantine, reserved names, signed-in owners only; aliases are guessable and the UI says so |
| Capture auth | Credential prompt phishing, timing | No `WWW-Authenticate`; MAC and constant-time comparison |
| View password | Brute force, grant leakage | argon2id, unlock rate limits, grants expire, query grants only on streams and never logged |
| Composer | Open proxy for attacks | Quotas per tier, guard, caller IP logged, response returned as data |
| Schedules | Scheduled flooding | Signed-in only, minimum interval, quotas, pause on failures |
| Export | CSV formula injection, archive paths | Neutralised cells; entry names from ids only |
| Import | JSON bombs | Size cap, nesting limit, per-capture validation |
| Subdomain capture | Cookie scope | `__Host-` cookie never reaches subdomains; `Set-Cookie` stays denied in responses |

## 33. Observability additions

| Metric | Type | Labels |
|---|---|---|
| `webhooks_live_captures_total` | counter | adds `result` = sender_refused, unauthorized, quota_exceeded; adds `kind` |
| `webhooks_live_rule_runs_total` | counter | `phase` = sync, async; `outcome` = succeeded, failed, overloaded, replayed |
| `webhooks_live_rule_sync_duration_seconds` | histogram | |
| `webhooks_live_rule_steps_total` | counter | `kind`, `outcome` |
| `webhooks_live_rule_queue_depth` | gauge | |
| `webhooks_live_script_runs_total` | counter | `outcome` = ok, operations, memory, time, error, panic |
| `webhooks_live_script_duration_seconds` | histogram | |
| `webhooks_live_template_renders_total` | counter | `outcome` |
| `webhooks_live_outbound_requests_total` | counter | `purpose` = forward, rule, schedule, composer, notify, store; `outcome` |
| `webhooks_live_quota_exceeded_total` | counter | `metric`, `tier` |
| `webhooks_live_notifications_total` | counter | `channel`, `outcome` = sent, failed, suppressed |
| `webhooks_live_mail_sessions` | gauge | |
| `webhooks_live_mail_messages_total` | counter | `result` = stored, refused_recipient, too_large, rate_limited, quota, store_unavailable |
| `webhooks_live_dns_queries_total` | counter | `result` = captured, answered, nxdomain, refused, rate_limited, dropped; `transport` |
| `webhooks_live_schedule_runs_total` | counter | `outcome` |
| `webhooks_live_exports_total`, `webhooks_live_export_bytes_total` | counter | `format`, `outcome` |
| `webhooks_live_token_auth_total` | counter | `result` = ok, invalid, expired, revoked |

Log fields added: `kind`, `rule_run_id`, `purpose`, `channel_kind`, `token_id` (never the token), `tier`. No
message content, secret, query string or header value is logged. Suggested alerts: rule queue depth above 1000
for 10 minutes; mail `store_unavailable` above zero for 5 minutes; DNS `rate_limited` ten times its weekly
baseline; any `script_runs_total{outcome="panic"}`.

## 34. Configuration additions

| Env | Default |
|---|---|
| `WEBHOOKS_LIVE_AUTH_MODE` | adds `mixed` |
| `WEBHOOKS_LIVE_CONFIG` `[tiers.*]` | built-in `anonymous` and `member` of §17 |
| `WEBHOOKS_LIVE_SECRETS_KEYS` | unset: secrets off |
| `WEBHOOKS_LIVE_API_CORS` | true |
| `WEBHOOKS_LIVE_RULES_ENABLED`, `WEBHOOKS_LIVE_RULES_SYNC_BUDGET`, `WEBHOOKS_LIVE_RULES_SYNC_CONCURRENCY`, `WEBHOOKS_LIVE_RULES_WORKERS` | true, `3s` (max `10s`), 256, 16 |
| `WEBHOOKS_LIVE_SCRIPT_THREADS`, `WEBHOOKS_LIVE_SCRIPT_MEMORY` | 2, 32 MiB |
| `WEBHOOKS_LIVE_VENDOR_PROXY` | unset |
| `WEBHOOKS_LIVE_SMTP_URL`, `WEBHOOKS_LIVE_NOTIFY_FROM` | unset: email notifications off |
| `WEBHOOKS_LIVE_VAPID_PRIVATE_KEY`, `WEBHOOKS_LIVE_VAPID_SUBJECT` | unset: push off |
| `WEBHOOKS_LIVE_MAIL_LISTEN_ADDR`, `WEBHOOKS_LIVE_MAIL_DOMAIN`, `WEBHOOKS_LIVE_MAIL_TRUSTED_PROXIES`, `WEBHOOKS_LIVE_MAIL_TLS_CERT`, `WEBHOOKS_LIVE_MAIL_TLS_KEY` | unset: mail off |
| `WEBHOOKS_LIVE_DNS_LISTEN_ADDR`, `WEBHOOKS_LIVE_DNS_ZONE`, `WEBHOOKS_LIVE_DNS_TRUSTED_PROXIES`, `WEBHOOKS_LIVE_DNS_ANSWER_A`, `WEBHOOKS_LIVE_DNS_ANSWER_AAAA`, `WEBHOOKS_LIVE_DNS_NS_NAME`, `WEBHOOKS_LIVE_DNS_SOA_EMAIL` | unset: DNS off |
| `WEBHOOKS_LIVE_SUBDOMAIN_CAPTURE_HOST` | unset |
| `WEBHOOKS_LIVE_SCHEDULES_ENABLED`, `WEBHOOKS_LIVE_EXPORT_CONCURRENCY`, `WEBHOOKS_LIVE_ALIAS_QUARANTINE` | true, 2, `30d` |

## 35. Delivery plan: parity milestones

Continues §14. Every milestone ends with its routes in the OpenAPI document (M21 adds the coverage test; later
milestones keep it green). Risky milestones go to `coder-pro` with an `xhigh` review.

| # | Milestone | Done when | Risk | After |
|---|---|---|---|---|
| M16 | Capture kinds: `captures` columns of §20, `kind` and `meta` in DTOs, `kind` list filter | Migration up and down; existing captures read as `http`; filter test | – | M6 |
| M17 | `mixed` mode and `Caller`: extractor, role table of §17, claim route, `creator_net`, listing in `mixed` | Role table tested per route class; two parallel claims, one wins | security | M8 (roles on routes) |
| M18 | Tiers and quotas: tier config, `users.tier`, `users set-tier`, `usage_counters` upsert, counted-object locks, `/usage`, per-caller `limits`, maintenance cleanup and `bytes_used` | 50 parallel increments never pass a limit; per-user storage refusal | concurrency | M17 |
| M19 | Secrets: `whl-secrets`, `endpoint_secrets`, routes, capture-auth MAC key, redaction, `secrets rotate` | Wrong AAD or key fails to open; no value in any response or log line (test scans); rotation re-seals | security | M17 |
| M20 | API tokens: table, routes (session only), Bearer extractor with scopes and endpoint limit, CSRF bypass for Bearer only, API CORS, tokens on streams | Scope matrix; revoked and expired 401; Bearer wins over cookie; preflight headers asserted, no `Allow-Credentials` | security | M17 |
| M21 | OpenAPI: `utoipa` on every existing DTO and handler, `/api/v1/openapi.json`, route coverage test | Coverage test green; document passes an OpenAPI 3.1 schema check | – | M20 |
| M22 | Organisation: tags, folders, capture note and star, ring skips starred, stats | Route tests; ring keeps starred | – | M16, M17 |
| M23 | Retention: per-endpoint retention and cap, never-expire count, capture TTL, eviction order, capture-insert condition | Maintenance test per case; capture insert honours per-endpoint values | concurrency | M18 |
| M24 | Aliases: table, routes, `/a/{alias}` capture routing with cache, reserved names, quarantine | Routing tests; quarantine blocks another user, not the same one | security | M18 |
| M25 | Capture access: sender allow list, capture auth (Basic, header), early 401 and 403 with `Connection: close`, protected preflights, metrics | Wire test per case; no `WWW-Authenticate`; MAC comparison constant-time | security, network input | M19, M24 |
| M26 | View password: argon2id, unlock with limits, grants, header and stream query grant, query-free logs | Limiter test; expiry; password change revokes grants | security | M17 |
| M27 | Clone and templates: configuration document, clone route, template routes, create from template | Clone equals source configuration without captures; template re-validated | – | M22 |
| M28 | `whl-template`: parser, roots, functions, filters, bounded writer, deadline, error positions | Test per filter and limit; property test output never exceeds caps; inserted `{{` stays text | security | – |
| M29 | Templated response and persistent variables: `response_templated`, render with fallback, `endpoint_vars` routes, atomic counter | Wire tests incl. invalid status fallback and a CR/LF header dropped; 50 parallel increments sum exactly | security, concurrency | M16, M25, M28 |
| M30 | Rules model and CRUD: `whl-rules` types and validation, `rules` table, routes, reorder, endpoint flags | Validation test per rule of api §15; flags rewritten in the same transaction | – | M28 |
| M31 | Rules before the response: plan cut, conditions, response, compute and state steps, budget, fallback header, `rule_runs` insert and post-sync write, rule-set cache, dry run, `rule_result` event | Endpoint without rules unchanged in latency; overrun falls back; expired `sync` lease replays without a response | concurrency | M29, M30 |
| M32 | Rules after the response: worker, per-step persistence, idempotent enqueue, `http`, `forward`, `store_s3`, retries, hop guard, re-run, quotas | Crash between steps enqueues nothing twice; self-calling rule stops at hop 3 | concurrency, security | M31, M18, M19 |
| M33 | Script sandbox: `whl-script` engine, limits, counting allocator, thread pool, host functions, script step in both parts, compile on save | Infinite loop, deep recursion, 100 MiB string, `eval` and `import` all stopped with named errors; same inputs give same output | sandboxing, security | M31 |
| M34 | Signature verifiers: schemes, presets, `captures.verification`, rule variable | Test vectors per preset computed locally; tampered body is `invalid` | security | M19, M29 |
| M35 | Notifications core: channels and routes, capture-trigger deliveries in the capture transaction, worker, webhook, Slack, Discord and Telegram adapters, host pinning, vendor proxy, limits with summaries, notify step, test route | Adapter bodies asserted against a local mock through a test-only host map; mentions neutralised; suppression then one summary | security, network | M32 |
| M36 | Email notifications: SMTP submission, address verification, one-click disable, caps | Flow tested with a local SMTP sink; unverified address refused | security | M35 |
| M37 | Web Push: VAPID, key route, subscriptions, encryption, host list, cleanup on 404 and 410 | Payload decrypts with test keys; foreign host refused | security | M35 |
| M38 | Composer: `/send`, self-target in process, history, quotas | SSRF cases refused; self-target yields a capture without network; history capped at 50 | security | M18 |
| M39 | Schedules: table, routes, cron and interval checks, claim loop, run to capture, pause, run now | Injected-clock tests: runs fire, no backfill, pause after 10 failures | concurrency | M16, M18, M19 |
| M40 | Export: four formats, selection route, size pre-check, concurrency caps, CSV neutralisation, gitops route timeout | 500 captures of 1 MiB export with RSS growth under 32 MiB; ZIP opens with `unzip`; formula cells neutralised | – | M16 |
| M41 | Import: streaming parse, validation, batches, capacity stop, `captures_imported` | Export then import round-trips content; malformed file rejected with its position | network input | M40 |
| M42 | Combined inbox: merged list and cursor, merged stream with vector resume | Two endpoints with interleaved captures resume without gaps | concurrency | M17, M22 |
| M43 | CLI forwarder: `whl-cli`, SSE client, state file, replay, backoff, release workflow | Server plus local target, CLI killed and restarted mid-stream, no gap | network | M20 |
| M44 | Mail receiver: PROXY v2 parser in `whl-core`, `whl-mail` listener, SMTP states, STARTTLS, recipient rules, limits, MIME to capture, parts route | Conversation tests per reply code with a real SMTP client; oversize refused; bad PROXY header closes; parts decode | network input, security | M16, M18 |
| M45 | Mail exposure: Envoy TCP listener, `TCPRoute`, PROXY policies, Service, MX, SPF, DMARC, edge port 25 | A message from a public mail provider arrives with its real client address | deploy | M44, owner |
| M46 | DNS responder: `whl-dns` UDP and TCP, PROXY v2, answers, rate limiting, negative cache, capture | `dig` tests over UDP and TCP; `ANY` minimal; flood stays within limits; case-randomised name captured | network input | M16, M18 |
| M47 | DNS exposure: dnsdist with suffix pool and PROXY, delegation and glue, public 53 | `dig` from outside creates a capture with the resolver address | deploy | M46, owner |
| M48 | Subdomain capture: host routing, wildcard certificate and listener | Capture via `<id>.<host>`; session cookie absent there | security | M25 |

| Order | Milestones | Note |
|---|---|---|
| 0 | M8 remainder (roles on routes) | Prerequisite for everything with users |
| 1 | M16, M28 | In parallel: one is schema, the other a pure crate |
| 2 | M17 | |
| 3 | M18, M19, M20 | In parallel (different crates and tables) |
| 4 | M21, M22, M23, M26, M38, M40, M44, M46 | In parallel; M44 and M46 are independent listeners |
| 5 | M24 → M25 → M29 → M30 → M31 → M32 | Serial: all but M30 edit the capture handler; M30 can overlap M29 |
| 6 | M33, M34, M35, M39, M41, M42, M43, M27 | In parallel after their inputs |
| 7 | M36, M37, M48 | |
| 8 | M45, M47 | After the owner's infrastructure decisions; can be prepared earlier |

## 36. Open questions for the owner (parity)

1. Who may sign in on the public instance in `mixed` mode? GlobalArt SSO with open registration, or another
   identity provider? Without public sign-in, every `member` feature stays out of reach of public users.
2. Mail: the mail domain (recommended a subdomain such as `email.webhooks.live`); is inbound port 25 allowed
   on the Moscow entry (option A of §28), or should ams1 relay (option B)?
3. DNS: the capture zone name (for example `dns.webhooks.live`); is a spare public IPv4 available for the DNS
   edge (option B of §29), or should dnsdist front PowerDNS on prod1-isp1 (option A)?
4. Outbound email for notifications: a Stalwart account and DKIM for the sending domain.
5. Vendor egress: which proxy reaches Discord and Telegram from prod1 (beg2's outbound proxy is a candidate).
6. Subdomain capture: a wildcard record and certificate for `*.webhooks.live`.
7. Tier numbers of §17 and whether `anonymous` keeps forwarding and chat notifications at all.
8. The secrets keyring in a SOPS secret, and who holds the rotation procedure.
