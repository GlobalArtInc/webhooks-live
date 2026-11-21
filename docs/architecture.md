# webhooks.live architecture

Status: draft 1, 2026-11-21. Requirements: [product-spec.md](product-spec.md). HTTP contract: [api.md](api.md).
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
  response_head bytea,
  PRIMARY KEY (job_id, attempt)
);
```

| Rule | Detail |
|---|---|
| Capture insert | One transaction: `UPDATE endpoints SET capture_count+1, bytes_used+n, last_seq+1, last_active_at=now() WHERE id=$1 AND deleted_at IS NULL AND last_active_at > now()-retention AND (capture_count < max OR overflow='ring') RETURNING last_seq, response_*`; ring mode then deletes `min(seq)` and corrects counters; `INSERT captures`, `INSERT capture_bodies`, `INSERT forward_jobs` per enabled rule, `pg_notify`. Zero rows from the `UPDATE` triggers one `SELECT` to tell gone (410) from full (410 with `X-Webhooks-Live-Error: endpoint_full`). |
| Ordering | `seq` is assigned under the endpoint row lock, so per-endpoint commit order equals `seq` order across replicas. It is the cursor and the SSE event id. `id` is a UUIDv7 (`X-Request-Id`, URLs). |
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
| Channel | `whl_events`, JSON `{"t":"capture","e":endpoint_id,"seq":n,"s":summary}`; also `capture_deleted`, `captures_cleared`, `endpoint_updated`, `endpoint_deleted`, `forward_result`. Path in the summary truncated to 1 KiB. |
| Listener | One `sqlx::postgres::PgListener` per replica on a dedicated connection. On reconnect it broadcasts `resync` to every local subscriber, since events may have been missed. |
| Hub | `HashMap<EndpointId, broadcast::Sender<Arc<Event>>>`, capacity 256, entry removed when the last receiver drops. Events for endpoints with no local viewer are discarded after decoding. |
| Slow viewer | A lagging receiver gets `RecvError::Lagged`, the stream sends `resync` and continues. Viewers never back-pressure the listener or the capture path. |
| Connect | Subscribe to the hub first, then read `seq > Last-Event-ID` from the database (at most 100 rows, ascending), then forward live events, skipping `seq` already replayed. More than 100 missing sends `resync`. Without `Last-Event-ID`, the stream starts with `ready` and live events only. |
| Liveness | Comment ping every 15 s, `retry: 2000`, server closes each stream after 30 minutes so clients rebalance across replicas. Connecting and every reconnect extend endpoint lifetime. |
| Shutdown | Streams receive `bye` and close; clients reconnect to another replica and resume by `Last-Event-ID`. |

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
| Config | `figment`: defaults, optional TOML (`WEBHOOKS_LIVE_CONFIG`), env `WEBHOOKS_LIVE_*` last | Used across GlobalArt Rust services; durations via `humantime`. |
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
| Result | Each attempt is a row in `forward_attempts` (status, duration, error class and message, final URL, first 4 KiB of response). Job end emits `forward_result` on `whl_events`. |
| Timeouts | Connect 5 s, total 15 s per attempt, response read capped at 64 KiB. |
| Concurrency | Per replica: 32 jobs in flight, 4 per target host (`Semaphore` map). |
| SSRF guard | Custom `reqwest::dns::Resolve` resolves, drops every address in a refused range, and hands only vetted addresses to the connector, so the checked address is the connected one (no rebinding). Literal-IP URLs are checked the same way. Refused: `0.0.0.0/8`, `10/8`, `100.64/10`, `127/8`, `169.254/16`, `172.16/12`, `192.0.0/24`, `192.168/16`, `198.18/15`, `224/4`, `240/4`, `255.255.255.255`, `::`, `::1`, `fc00::/7`, `fe80::/10`, `ff00::/8`, `64:ff9b::/96`, IPv4-mapped and IPv4-compatible forms of the above. `WEBHOOKS_LIVE_FORWARD_ALLOW_CIDRS` re-opens ranges. Schemes `http`/`https` only. |
| Redirects | Automatic redirects off; the worker follows up to 5 hops itself, re-validating scheme and running each hop through the same resolver. 307/308 keep method and body; 301/302/303 become `GET` without body. |
| Proxy | System proxy settings ignored (`no_proxy`), because a proxy would resolve names itself and bypass the guard. |
| Manual resend | API creates a `manual` job with the same guard; the result arrives as `forward_result`. |

## 7. Auth

**Decision: `WEBHOOKS_LIVE_AUTH_MODE=open|sso`. In `sso`, OIDC authorization code with PKCE against the operator's
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
| Ownership | `endpoints.owner_id` = creator. Shares in `endpoint_members` with `viewer` or `editor`. Shared by email of a user who has signed in at least once. |
| Roles | Owner: everything. Editor: update response and forwarding, delete captures, resend. Viewer: read and stream. Endpoint delete and member management: owner only. Non-members get 404, never 403, so ids do not leak. |
| Who may sign in | Decided by the IdP client (GlobalArt SSO group enforcement); webhooks.live trusts any valid ID token from the issuer. |
| Machine clients | Not in the first release; `Authorization: Bearer` is reserved for personal tokens (open question 3). |

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
| Stored XSS, raw body | `GET .../body` always answers `application/octet-stream`, or `text/plain; charset=utf-8` when `?view=text`, with `X-Content-Type-Options: nosniff`, `Content-Security-Policy: sandbox; default-src 'none'`, `Cross-Origin-Resource-Policy: same-origin`, and `Content-Disposition: attachment` when `?download=1`. The captured `Content-Type` is never echoed. The UI renders bodies as text through React escaping, never `dangerouslySetInnerHTML`; any HTML preview is an `<iframe sandbox>` without `allow-scripts`. |
| Stored XSS, capture response | The configured response is served from the UI origin, so every capture response carries `Content-Security-Policy: sandbox` and `nosniff`: the document gets an opaque origin, no cookie or same-origin API access. |
| Header injection | Configured response headers: at most 20; name an RFC 9110 token up to 64 bytes; value visible ASCII, space and tab, up to 1024 bytes, no CR, LF or NUL. Denied names: `Content-Length`, `Transfer-Encoding`, `Connection`, `Keep-Alive`, `Upgrade`, `Trailer`, `TE`, `Host`, `Proxy-*`, `Set-Cookie`, `Content-Security-Policy*`, `X-Content-Type-Options`, `Access-Control-*`, `X-Request-Id`, `X-Endpoint-Id`, `Content-Type` (has its own field). `Set-Cookie` is denied because a capture response could toss a `__Host-` session cookie on the shared origin. |
| Content type | Parsed as a MIME type (`type/subtype` plus parameters), up to 255 bytes, no control characters. |
| SSRF | §6. |
| Smuggling and parsing | Hyper rejects conflicting `Content-Length`/`Transfer-Encoding`, obs-fold and malformed chunking; webhooks.live only listens behind Envoy; early responses close the connection (§5); the forwarder builds fresh requests and never relays raw bytes or hop-by-hop headers. |
| CSRF | No CORS on `/api` and `/auth`. In `sso`, mutating requests carrying the session cookie must have `Origin` equal to `WEBHOOKS_LIVE_PUBLIC_URL`'s origin (else 403 `csrf_rejected`); request bodies must be `application/json` (else 415); cookie is `SameSite=Lax`. In `open` there is no ambient credential, so no CSRF surface. |
| Id leakage | UI sends `Referrer-Policy: no-referrer`; logs carry endpoint and capture ids but never header values or bodies. |
| UI headers | `Content-Security-Policy: default-src 'self'; img-src 'self' data:; style-src 'self'; frame-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'`, `X-Frame-Options: DENY`. |

## 10. Frontend delivery and path layout

**Decision: the built React app is embedded in the binary with `rust-embed` and served by `whl-web` on
the same origin.** One artefact, UI and API versions never skew, no CORS. Rejected: a separate static
server or container (two images to version, plus cross-origin cookies).

| Path | Served |
|---|---|
| `/{endpoint_id}`, `/{endpoint_id}/{*rest}` | Capture (24 chars, Crockford base32 lowercase) |
| `/api/v1/...` | Management API and `/api/v1/endpoints/{id}/stream` |
| `/auth/login`, `/auth/callback`, `/auth/logout` | OIDC |
| `/assets/{file}` | Hashed build output: `Cache-Control: public, max-age=31536000, immutable` |
| `/`, `/e/{endpoint_id}`, `/e/{endpoint_id}/c/{capture_id}`, `/settings` | `index.html`, `Cache-Control: no-cache` |
| `/favicon.ico`, `/robots.txt` | Static, `max-age=86400`; `robots.txt` disallows everything |
| anything else | 404 |

Endpoint id: 15 bytes from `OsRng` (120 bits, on par with UUID v4's 122), Crockford base32 lowercase
without padding, exactly 24 characters (alphabet `0-9a-z` minus `i l o u`). No UI or API root reaches 24
characters, and deep links use the `/e/` prefix, so a root segment is either a capture id or a fixed route.
The UI is fetched from `index.html` for known UI prefixes only; unknown paths stay 404 so typos in capture
URLs are visible to senders. Release builds fail if `web/dist` is missing; debug builds read from disk.

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

| Env | Default |
|---|---|
| `WEBHOOKS_LIVE_PUBLIC_URL` | required |
| `WEBHOOKS_LIVE_DATABASE_URL`, `WEBHOOKS_LIVE_DB_POOL_SIZE` | required, 20 |
| `WEBHOOKS_LIVE_LISTEN_ADDR`, `WEBHOOKS_LIVE_ADMIN_ADDR` | `0.0.0.0:8080`, `0.0.0.0:9090` |
| `WEBHOOKS_LIVE_AUTH_MODE` | `open` |
| `WEBHOOKS_LIVE_OIDC_ISSUER`, `WEBHOOKS_LIVE_OIDC_CLIENT_ID`, `WEBHOOKS_LIVE_OIDC_CLIENT_SECRET`, `WEBHOOKS_LIVE_OIDC_SCOPES` | required in `sso`; scopes `openid email profile` |
| `WEBHOOKS_LIVE_SESSION_IDLE`, `WEBHOOKS_LIVE_SESSION_MAX` | `12h`, `7d` |
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
| M8 | `sso`: OIDC, sessions, origin check, roles on every route, members API, endpoint listing | tests against a mock issuer; non-member gets 404 | security |
| M9 | Rate limiters, storage cap flag, maintenance job, sliding expiry, purge | eviction and cap tests; job runs on one instance only | concurrency |
| M10 | SSRF-guarded client: resolver, ranges, redirects, timeouts | unit tests per range and redirect case | security, network |
| M11 | Forward queue: jobs in capture transaction, claim loop, retries, attempts, manual resend, `forward_result` | integration tests incl. lease reclaim | concurrency |
| M12 | `whl-web`: embedding, cache headers, UI security headers, SPA fallback | route table of §10 asserted | – |
| M13 | Deploy: gitops HelmRelease, route, ImagePolicy, migrate job, OIDC client URIs, `webhooks-live.globalart.hu` | side-by-side instance live | deploy (`xhigh` review) |
| M14 | Load scripts and Playwright scenarios | targets of §13 met on the side-by-side instance | – |
| M15 | Cutover steps 2–3 | old stack removed | deploy |

The React UI itself is planned from `ui-design.md`; it can start after M3 against api.md.

## 15. Open questions for the owner

1. Which prod1 Postgres cluster hosts the `webhooks_live` database, and the storage cap (20 GiB assumed).
2. Do pods in `webhooks-live-prod` have direct internet egress for forwarding? If prod1 requires the egress proxy,
   forwarding needs a CONNECT-to-vetted-IP mode (one more milestone) to keep the SSRF guard intact.
3. Personal API tokens for the local-forwarding CLI in `sso` mode: first release or later?
4. Keep the OPS OIDC client (Ops group only, as today) or create a dedicated webhooks.live client with wider access?
