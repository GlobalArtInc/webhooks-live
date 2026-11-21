# webhooks.live HTTP API

Status: draft 1, 2026-11-21. Requirements: [product-spec.md](product-spec.md). Design: [architecture.md](architecture.md).
This document is the contract between the Rust backend and the React frontend.

## 1. Conventions

| Topic | Rule |
|---|---|
| Base | Management API under `/api/v1`, same origin as the UI. JSON in and out, UTF-8. |
| Request bodies | `Content-Type: application/json` required on `POST`/`PUT`/`PATCH` with a body, else 415. Unknown fields are rejected (422). |
| Timestamps | RFC 3339, UTC, millisecond precision: `2026-11-21T10:15:30.123Z`. |
| Ids | Endpoint id: 24 chars `[0-9a-hjkmnp-tv-z]`. Capture, job, user ids: UUID, lowercase hyphenated. |
| Bytes in JSON | `Body` object: `{"encoding": "utf8" \| "base64", "data": string, "truncated": bool}`. `utf8` when the bytes are valid UTF-8, else `base64`. |
| Name/value lists | Headers, query, form fields: ordered arrays of `{"name": string, "value": string}`, duplicates kept. |
| Absent vs null | In `PATCH`, an absent field is unchanged; `null` resets a nullable field. |
| Auth, `open` mode | None. Knowing the endpoint id grants access. |
| Auth, `sso` mode | Cookie `__Host-whl_session` set by `/auth/callback`. Missing or expired: 401 `unauthenticated`; the UI sends the browser to `/auth/login?return_to=<current path>`. |
| CSRF, `sso` mode | Mutating requests with the session cookie must send `Origin` equal to the public origin, else 403 `csrf_rejected`. Browsers do this automatically for `fetch`. |
| Access | In `sso`, an endpoint the caller has no role on answers 404 `endpoint_not_found`, never 403. |
| Rate limits | Every `/api` response: `RateLimit-Limit` (burst size), `RateLimit-Remaining`. 429 adds `Retry-After` (seconds). Limits are per replica. |
| Caching | All `/api` responses: `Cache-Control: no-store`. |

### 1.1 Error body

```json
{
  "error": {
    "code": "validation_failed",
    "message": "response.status must be between 100 and 599",
    "fields": [{ "field": "response.status", "message": "must be between 100 and 599" }]
  }
}
```

`fields` is present only for `validation_failed`. Clients branch on `code`, never on `message`.

| Status | `code` | When |
|---|---|---|
| 400 | `invalid_json` | Body is not parseable JSON |
| 400 | `invalid_cursor` | Cursor malformed or used with different order or filters |
| 401 | `unauthenticated` | `sso`, no valid session |
| 403 | `csrf_rejected` | Origin check failed |
| 403 | `forbidden` | Role too low (viewer tries to edit, editor tries to delete the endpoint) |
| 403 | `not_available_in_mode` | Route needs `sso` (listing, members) |
| 404 | `endpoint_not_found`, `capture_not_found`, `user_not_found`, `not_found` | Missing, expired, deleted, or no access |
| 413 | `payload_too_large` | Request body over 256 KiB |
| 415 | `unsupported_media_type` | Body without `application/json` |
| 422 | `validation_failed` | Field rules in §4 |
| 422 | `target_refused` | Forward target is not `http`/`https` or resolves only to refused addresses |
| 429 | `rate_limited` | Limiter hit |
| 503 | `store_unavailable` | Database timeout or outage; `Retry-After: 5` |

### 1.2 Pagination

Cursor-based. A list response has `next_cursor` (string or `null`). Pass it back as `cursor` with the same
`order` and filters; the cursor is opaque. `limit` default 50, max 200.

## 2. Path layout

| Path | Purpose |
|---|---|
| `/{endpoint_id}`, `/{endpoint_id}/{*rest}` | Capture, any method, public |
| `/api/v1/...` | Management API |
| `/api/v1/endpoints/{endpoint_id}/stream` | Live stream (SSE) |
| `/auth/login`, `/auth/callback`, `/auth/logout` | OIDC, `sso` only |
| `/`, `/e/{endpoint_id}`, `/e/{endpoint_id}/c/{capture_id}`, `/settings` | UI pages (deep links, V6) |
| `/assets/...`, `/favicon.ico`, `/robots.txt` | Static |
| anything else | 404 |

## 3. Capture path

`ANY /{endpoint_id}[/{rest}][?query]`. Not under `/api`; responses are for senders, so errors are plain text.

| Case | Status | Body |
|---|---|---|
| Stored | Configured status, or the override when the first segment of `rest` is `100`–`599` | Configured body, configured content type |
| CORS mode, preflight (`OPTIONS` + `Origin` + `Access-Control-Request-Method`) | 204 | empty (also stored) |
| Malformed id | 404 | `Not found.` |
| Unknown, expired or deleted endpoint | 410 | `This endpoint does not exist or has expired. Create a new one at <public url>.` |
| Endpoint full, `refuse` mode | 410 | `This endpoint holds the maximum of 500 captures. Delete captures to receive more.` |
| Body over the limit | 413 | `Request body exceeds 10485760 bytes.` |
| Rate limited | 429 | `Too many requests.` plus `Retry-After` |
| Store unavailable | 503 | `Temporarily unavailable.` plus `Retry-After: 5` |
| Global storage full | 507 | `Storage is full. Try again later.` |

| Response header | When | Value |
|---|---|---|
| `X-Request-Id` | Stored | Capture id |
| `X-Endpoint-Id` | Stored, 410 | Endpoint id |
| `X-Webhooks-Live-Error` | Every non-stored answer | `malformed_id`, `endpoint_gone`, `endpoint_full`, `too_large`, `rate_limited`, `store_unavailable`, `storage_full` |
| `Content-Security-Policy` | Always | `sandbox` |
| `X-Content-Type-Options` | Always | `nosniff` |
| `Cache-Control` | Always | `no-store` |
| Configured headers | Stored | From `response.headers` |
| `Access-Control-Allow-Origin` | CORS mode | `*` |
| `Access-Control-Expose-Headers` | CORS mode | `X-Request-Id, X-Endpoint-Id` |
| `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers`, `Access-Control-Max-Age` | CORS preflight | Requested method, requested headers, `600` |

The response is sent after the configured `delay_ms`; the capture is already stored and announced by then.

## 4. Objects

### 4.1 Endpoint

| Field | Type | Notes |
|---|---|---|
| `id` | string | 24 chars |
| `url` | string | Capture URL, `<public url>/<id>` |
| `title` | string \| null | Up to 100 chars |
| `response.status` | integer | 100–599, default 200 |
| `response.content_type` | string | MIME type, up to 255 bytes, default `text/plain` |
| `response.headers` | `{name, value}[]` | Up to 20; rules in architecture §9 |
| `response.body` | Body | Up to 64 KiB decoded; `truncated` always false; default empty `utf8` |
| `response.delay_ms` | integer | 0–10000, default 0 |
| `cors` | boolean | Default false |
| `overflow` | `"refuse"` \| `"ring"` | Default `refuse` |
| `forward_rules` | ForwardRule[] | Up to 5 |
| `max_captures` | integer | Read-only, instance setting |
| `capture_count` | integer | Read-only |
| `bytes_used` | integer | Read-only |
| `last_seq` | integer | Read-only; `seq` of the newest capture ever stored |
| `created_at`, `last_active_at`, `expires_at` | timestamp | Read-only; `expires_at` = `last_active_at` + retention |
| `owner` | User \| null | Read-only; `null` in `open` |
| `my_role` | `"owner"` \| `"editor"` \| `"viewer"` \| null | Read-only; `null` in `open` |

**EndpointInput** (create and `PATCH`): `title`, `response` (any subset of its fields), `cors`, `overflow`,
`forward_rules`. `forward_rules` replaces the whole list.

### 4.2 ForwardRule

| Field | Type | Notes |
|---|---|---|
| `id` | uuid | Server-assigned; send it back to keep a rule's identity |
| `enabled` | boolean | Default true |
| `target_url` | string | `http`/`https`, up to 2048 chars |
| `method` | string \| null | `null` keeps the captured method |
| `content_type` | string \| null | `null` keeps the captured type |
| `header_allowlist` | string[] | Captured headers to copy, case-insensitive, up to 50 |
| `append_path` | boolean | Append captured sub-path and query to `target_url`, default false |

### 4.3 CaptureSummary (list items and stream payloads)

| Field | Type | Notes |
|---|---|---|
| `id` | uuid | Same as sender's `X-Request-Id` |
| `seq` | integer | Per-endpoint, increasing in arrival order |
| `endpoint_id` | string | |
| `received_at` | timestamp | |
| `method` | string | |
| `path` | string | Sub-path after the id, starts with `/` or is empty; up to 1024 chars in stream payloads |
| `query_string` | string | Raw, without `?` |
| `client_ip` | string | Resolved sender IP |
| `content_type` | string \| null | |
| `body_size` | integer | Bytes |
| `status_override` | integer \| null | From the sub-path |
| `response_status` | integer | Status sent to the sender |
| `forward_state` | `"pending"` \| `"succeeded"` \| `"failed"` \| null | Worst state among the capture's jobs; `null` if none |

### 4.4 Capture (detail) = CaptureSummary plus

| Field | Type | Notes |
|---|---|---|
| `url` | string | Full URL as seen by the server |
| `host` | string | |
| `headers` | `{name, value}[]` | Original order |
| `query` | `{name, value}[]` | |
| `form` | `{fields: {name, value}[], files: {name, filename, content_type, size}[]}` \| null | Form content types only |
| `body` | Body | First 1 MiB; `truncated: true` when larger, fetch the rest from `/body` |
| `forwards` | ForwardJob[] | Newest first |

### 4.5 ForwardJob

| Field | Type |
|---|---|
| `id` | uuid |
| `kind` | `"rule"` \| `"manual"` |
| `rule_id` | uuid \| null |
| `target_url`, `method` | string |
| `state` | `"pending"` \| `"running"` \| `"succeeded"` \| `"failed"` |
| `attempts` | `{attempt: integer, started_at: timestamp, duration_ms: integer, status: integer \| null, error: string \| null, final_url: string \| null}[]` |
| `created_at` | timestamp |
| `finished_at` | timestamp \| null |

`error` is one of `timeout`, `connect_error`, `tls_error`, `blocked_address`, `too_many_redirects`,
`invalid_redirect`, followed by `: ` and a short detail.

### 4.6 User and Member

User: `{id: uuid, email: string, name: string | null}`. Member: `{user: User, role: "viewer" | "editor", added_at: timestamp}`.

## 5. Routes

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/instance` | any | – | 200 Instance |
| `POST /api/v1/endpoints` | any (`sso`: signed in) | EndpointInput or empty body | 201 Endpoint, `Location: /api/v1/endpoints/{id}` |
| `GET /api/v1/endpoints` | signed in, `sso` only | `?cursor&limit&role=owner\|shared` | 200 `{items: Endpoint[], next_cursor}`, by `last_active_at` desc |
| `GET /api/v1/endpoints/{id}` | viewer | – | 200 Endpoint |
| `PATCH /api/v1/endpoints/{id}` | editor | EndpointInput | 200 Endpoint |
| `DELETE /api/v1/endpoints/{id}` | owner | – | 204 |
| `GET /api/v1/endpoints/{id}/members` | owner, `sso` | – | 200 `{items: Member[]}` |
| `PUT /api/v1/endpoints/{id}/members` | owner, `sso` | `{email: string, role: "viewer" \| "editor"}` | 200 Member (create or change role) |
| `DELETE /api/v1/endpoints/{id}/members/{user_id}` | owner, `sso` | – | 204 |
| `GET /api/v1/endpoints/{id}/captures` | viewer | query below | 200 `{items: CaptureSummary[], next_cursor, total}` |
| `GET /api/v1/endpoints/{id}/captures/latest` | viewer | – | 200 Capture, 404 `capture_not_found` if empty |
| `GET /api/v1/endpoints/{id}/captures/{capture_id}` | viewer | – | 200 Capture |
| `GET /api/v1/endpoints/{id}/captures/{capture_id}/body` | viewer | `?view=text&download=1` | 200 raw bytes, §5.2 |
| `DELETE /api/v1/endpoints/{id}/captures/{capture_id}` | editor | – | 204 |
| `DELETE /api/v1/endpoints/{id}/captures` | editor | – | 204 |
| `POST /api/v1/endpoints/{id}/captures/{capture_id}/forward` | editor | `{target_url, method?, content_type?, header_allowlist?}` | 202 ForwardJob (`state: "pending"`) |
| `GET /api/v1/endpoints/{id}/stream` | viewer | `Last-Event-ID` header or `?last_event_id=` | 200 `text/event-stream`, §6 |

Reading an endpoint, its captures or its stream extends its lifetime (E3). `PUT .../members` with an email
nobody has signed in with answers 404 `user_not_found`; the owner cannot add themself (422).

**Instance**

```json
{
  "version": "1.0.0",
  "auth_mode": "sso",
  "user": { "id": "0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21", "email": "dev@example.com", "name": "Dev" },
  "public_url": "https://webhooks.globalart.hu",
  "limits": {
    "max_captures": 500,
    "max_body_bytes": 10485760,
    "retention_seconds": 604800,
    "max_delay_ms": 10000,
    "max_forward_rules": 5
  },
  "features": { "forwarding": true }
}
```

`user` is `null` when signed out or in `open` mode.

### 5.1 Capture list query

| Param | Type | Meaning |
|---|---|---|
| `order` | `desc` \| `asc` | By `seq`; default `desc` |
| `limit` | integer | 1–200, default 50 |
| `cursor` | string | From `next_cursor` |
| `method` | string | Comma list, e.g. `POST,PUT` |
| `status` | string | Comma list of override classes `1xx`…`5xx`, or `none` |
| `q` | string | Case-insensitive substring in path, query, headers, first 256 KiB of body; 1–200 chars |
| `since`, `until` | timestamp | `received_at` range, inclusive start, exclusive end |

`total` is the number of captures matching the filters, ignoring the cursor.

### 5.2 Raw body response

| Header | Value |
|---|---|
| `Content-Type` | `application/octet-stream`; `text/plain; charset=utf-8` with `view=text` |
| `Content-Disposition` | `attachment; filename="<capture_id>.bin"` with `download=1`, else `inline` |
| `Content-Security-Policy` | `sandbox; default-src 'none'` |
| `X-Content-Type-Options` | `nosniff` |
| `Cross-Origin-Resource-Policy` | `same-origin` |
| `Content-Length` | `body_size` |

The captured content type is never used as the response type.

## 6. Live stream

`GET /api/v1/endpoints/{id}/stream`, `Accept: text/event-stream`. Response headers: `Content-Type:
text/event-stream`, `Cache-Control: no-store`, `X-Accel-Buffering: no`. The server sends `retry: 2000`,
a `: ping` comment every 15 s, and closes the stream after 30 minutes; `EventSource` reconnects by itself.

| Event | `id` | `data` | Client action |
|---|---|---|---|
| `ready` | – | `{"endpoint_id": string, "last_seq": integer}` | First event of every connection |
| `capture` | `seq` | CaptureSummary | Prepend to list; fetch detail on demand |
| `capture_deleted` | – | `{"capture_id": uuid, "seq": integer}` | Remove from list |
| `captures_cleared` | – | `{}` | Empty the list |
| `endpoint_updated` | – | Endpoint | Replace settings |
| `endpoint_deleted` | – | `{}` | Show gone state, stop |
| `forward_result` | – | `{"capture_id": uuid, "job": ForwardJob}` | Update forward state |
| `resync` | – | `{"reason": "lagged" \| "gap" \| "listener_reconnect"}` | Re-list from the top |
| `bye` | – | `{}` | Server shutting down; let `EventSource` reconnect |

Resume: `EventSource` sends `Last-Event-ID` (the last `capture` `seq`) on reconnect. The server replays
missed `capture` events in ascending `seq` before live ones, up to 100; beyond that it sends `resync`. A page
that already holds a list opens the stream with `?last_event_id=<highest seq it has>` to close the gap between
its list request and the stream. Live delivery is best-effort: `resync` means "list again".

## 7. Auth routes (`sso` mode)

| Method, path | Behaviour |
|---|---|
| `GET /auth/login?return_to=/e/{id}` | 302 to the issuer. `return_to` must start with `/` and not `//`; else `/`. |
| `GET /auth/callback?code&state` | Validates, creates the session, sets `__Host-whl_session`, 302 to `return_to`. Failure: 302 to `/?login_error=<code>`. |
| `POST /auth/logout` | Origin-checked; deletes the session, clears the cookie, 204. |

In `open` mode these answer 404.

## 8. Examples

```sh
curl -s -X POST https://webhooks.globalart.hu/api/v1/endpoints \
  -H 'Content-Type: application/json' \
  -d '{"title":"stripe test","response":{"status":202,"content_type":"application/json","body":{"encoding":"utf8","data":"{\"ok\":true}"},"delay_ms":500},"cors":true}'
```

```json
{
  "id": "7k2m9q4xw8c1d5f3h6j0n2p4",
  "url": "https://webhooks.globalart.hu/7k2m9q4xw8c1d5f3h6j0n2p4",
  "title": "stripe test",
  "response": {
    "status": 202,
    "content_type": "application/json",
    "headers": [],
    "body": { "encoding": "utf8", "data": "{\"ok\":true}", "truncated": false },
    "delay_ms": 500
  },
  "cors": true,
  "overflow": "refuse",
  "forward_rules": [],
  "max_captures": 500,
  "capture_count": 0,
  "bytes_used": 0,
  "last_seq": 0,
  "created_at": "2026-11-21T10:15:30.123Z",
  "last_active_at": "2026-11-21T10:15:30.123Z",
  "expires_at": "2026-11-28T10:15:30.123Z",
  "owner": null,
  "my_role": null
}
```

```sh
curl -i -X POST https://webhooks.globalart.hu/7k2m9q4xw8c1d5f3h6j0n2p4/404/orders?x=1 \
  -H 'Content-Type: application/json' -d '{"order":42}'

curl -s 'https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures?method=POST&q=order&limit=20'

curl -s https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures/latest

curl -s -o body.bin 'https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures/0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b/body?download=1'

curl -s -X PATCH https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4 \
  -H 'Content-Type: application/json' \
  -d '{"forward_rules":[{"target_url":"https://api.example.com/hooks","header_allowlist":["X-Signature"],"append_path":true}]}'

curl -s -X POST https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures/0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b/forward \
  -H 'Content-Type: application/json' -d '{"target_url":"https://staging.example.com/hooks"}'

curl -N -H 'Last-Event-ID: 41' https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/stream
```

Stream output:

```
retry: 2000

event: ready
data: {"endpoint_id":"7k2m9q4xw8c1d5f3h6j0n2p4","last_seq":42}

id: 42
event: capture
data: {"id":"0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b","seq":42,"endpoint_id":"7k2m9q4xw8c1d5f3h6j0n2p4","received_at":"2026-11-21T10:16:02.511Z","method":"POST","path":"/404/orders","query_string":"x=1","client_ip":"203.0.113.7","content_type":"application/json","body_size":12,"status_override":404,"response_status":404,"forward_state":null}

: ping
```

In `sso` mode the same calls need the session cookie, and mutating ones an `Origin` header; machine access
in `sso` mode is not part of the first release.
