# webhooks.live HTTP API

Status: draft 2, 2026-11-21 (parity routes §9–§24). Requirements: [product-spec.md](product-spec.md). Design: [architecture.md](architecture.md).
This document is the contract between the Rust backend and the React frontend.

## 1. Conventions

| Topic | Rule |
|---|---|
| Base | Management API under `/api/v1`, same origin as the UI. JSON in and out, UTF-8. |
| Request bodies | `Content-Type: application/json` required on `POST`/`PUT`/`PATCH` with a body, else 415. The body must be a JSON object, else 400 `invalid_json`. A `PATCH` with an empty body is 400 `invalid_json`; `POST /api/v1/endpoints` accepts an empty body. Limit: 256 KiB of request body as sent (base64 inflation counts), else 413; a decoded `response.body` may hold at most 64 KiB. |
| Timestamps | RFC 3339, UTC, millisecond precision: `2026-11-21T10:15:30.123Z`. |
| Ids | Endpoint id: 24 chars `[0-9a-hjkmnp-tv-z]`. Capture, job, user ids: UUID, lowercase hyphenated. |
| Bytes in JSON | `Body` object: `{"encoding": "utf8" \| "base64", "data": string, "truncated": bool}`. `utf8` when the bytes are valid UTF-8, else `base64`. `truncated` is true when `data` holds only a prefix; each use states its prefix size. In requests `truncated` may be omitted and is ignored. |
| Name/value lists | Ordered arrays of `{"name": string, "value": string}`. Captured request headers, query and form fields keep duplicates. Configured `response.headers` reject a name repeated case-insensitively (field code `duplicate`). |
| Absent vs null | In `PATCH`, an absent field is unchanged; `null` resets a nullable field. |
| Auth, `open` mode | None. Knowing the endpoint id grants access. |
| Auth, `sso` mode | Cookie `__Host-whl_session` set by `/auth/callback`. Missing or expired: 401 `unauthenticated`; the UI sends the browser to `/auth/login?return_to=<current path>`. |
| CSRF, `sso` mode | Mutating requests with the session cookie must send `Origin` equal to the public origin, else 403 `csrf_rejected`. Browsers do this automatically for `fetch`. |
| Access | Unknown, expired and deleted endpoints answer 410 `endpoint_gone`; in `sso` so does an endpoint the caller has no role on, never 403. A malformed endpoint id answers 404 `not_found`. |
| Current user | There is no `/me` route. `GET /api/v1/instance` carries `user` (`null` when anonymous or in `open`); the UI fetches it at start and after login or logout. |
| Rate limits | First release: management routes send no rate-limit headers until milestone M9. From M9 every `/api` response carries `RateLimit-Limit` (burst size) and `RateLimit-Remaining`; 429 adds `Retry-After` (seconds). Limits are per replica. |
| Caching | All `/api` responses: `Cache-Control: no-store`. |

### 1.1 Error body

```json
{
  "error": {
    "code": "validation_failed",
    "message": "response.status must be between 100 and 599",
    "fields": [{ "field": "response.status", "code": "out_of_range", "message": "must be between 100 and 599" }]
  }
}
```

`fields` is present only for `validation_failed`; all violations are reported together, up to 50. `field` is a
dotted path into the request body, with array indexes as numbers (`response.headers.2.name`). The top-level
`message` of `validation_failed` is `"<first field path> <its message>"`. Clients branch on `code`, never on
`message`.

Field `code` is a closed list:

| Field code | Raised for |
|---|---|
| `required` | Missing or empty value |
| `invalid_format` | Wrong JSON type, control character, non-ASCII where ASCII is required, invalid token, invalid media type, malformed URL |
| `too_long` | Over the length limit |
| `out_of_range` | Number outside its range |
| `too_many` | Array over its item limit |
| `duplicate` | Response header name repeated (case-insensitive) |
| `not_allowed` | Denied header name, unknown JSON field, `overflow: ring` when not allowed |
| `target_refused` | Forward target refused by the SSRF rules |
| `target_unresolvable` | Forward target host does not resolve |

| Status | `code` | When |
|---|---|---|
| 400 | `invalid_json` | Body is not parseable JSON, is not a JSON object, or is empty on `PATCH` |
| 400 | `invalid_cursor` | Cursor malformed or used with different order or filters |
| 401 | `unauthenticated` | `sso`, no valid session |
| 403 | `csrf_rejected` | Origin check failed |
| 403 | `forbidden` | Role too low (viewer tries to edit, editor tries to delete the endpoint) |
| 403 | `not_available_in_mode` | Route needs `sso` or `mixed` (listing, members, claim) |
| 404 | `capture_not_found` | Capture id unknown in this endpoint |
| 404 | `forward_rule_not_found` | Rule id unknown in this endpoint |
| 404 | `user_not_found` | `PUT .../members` with an email nobody has signed in with |
| 404 | `member_not_found` | `DELETE .../members/{user_id}` for a user who is not a member (including unknown ids and the owner) |
| 404 | `not_found` | Unknown route or malformed id |
| 405 | `method_not_allowed` | Known path, unsupported method; `Allow` lists the supported ones |
| 410 | `endpoint_gone` | Endpoint unknown, expired or deleted, or (`sso`) no role on it |
| 413 | `payload_too_large` | Request body over 256 KiB as sent |
| 415 | `unsupported_media_type` | Body without `application/json` |
| 422 | `validation_failed` | Field rules in §4, including SSRF refusal of a forward target (field code `target_refused`) |
| 422 | `forward_rule_limit` | Endpoint already has the maximum number of rules |
| 429 | `rate_limited` | Limiter hit |
| 500 | `internal_error` | Unexpected server failure; details only in server logs |
| 503 | `store_unavailable` | Database timeout or outage; `Retry-After: 5` |
| 503 | `auth_unavailable` | `sso`: the identity provider was unreachable at start and discovery is still being retried; only `/auth/login` and `/auth/callback` answer it; `Retry-After: 30` |
| 503 | `login_busy` | `sso`: too many sign-ins are pending (`WEBHOOKS_LIVE_AUTH_MAX_PENDING_LOGINS`); `/auth/login` only; `Retry-After: 30` |

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
| `/`, `/e/{endpoint_id}` and its sub-routes, `/endpoints`, `/signin`, `/settings` | UI pages (deep links, V6); full list in ui-design.md §2 |
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
| `url` | string | Capture URL: `public_url` without trailing slash, plus `/<id>` |
| `title` | string \| null | Up to 100 chars |
| `response.status` | integer | 100–599, default 200 |
| `response.content_type` | string | MIME type, up to 255 bytes, default `text/plain` |
| `response.headers` | `{name, value}[]` | Up to 20; no name twice (case-insensitive); denylist in architecture §9 |
| `response.body` | Body | Up to 64 KiB decoded; `truncated` always false; default empty `utf8` |
| `response.delay_ms` | integer | 0–10000, default 0 |
| `cors` | boolean | Default false |
| `overflow` | `"refuse"` \| `"ring"` | Default `refuse`; `ring` rejected (field code `not_allowed`) when `features.ring_mode_allowed` is false |
| `forward_rules` | ForwardRule[] | Read-only here, ordered by creation; edited through §5.3 |
| `max_captures` | integer | Read-only, instance setting |
| `capture_count` | integer | Read-only |
| `bytes_used` | integer | Read-only |
| `last_seq` | integer | Read-only; `seq` of the newest capture ever stored |
| `created_at`, `last_active_at`, `expires_at` | timestamp | Read-only; `expires_at` = `last_active_at` + retention |
| `owner` | User \| null | Read-only; `null` in `open` |
| `my_role` | `"owner"` \| `"editor"` \| `"viewer"` \| null | Read-only; `null` in `open` |

**EndpointInput** (create and `PATCH`): `title`, `response` (any subset of its fields), `cors`, `overflow`.
Forward rules are not part of it.

### 4.2 ForwardRule

| Field | Type | Notes |
|---|---|---|
| `id` | uuid | Read-only, server-assigned |
| `enabled` | boolean | Default true |
| `target_url` | string | Required on create; `http`/`https`, up to 2048 chars; checked as in §5.3 |
| `method` | string \| null | Default `null` = keep the captured method; else an RFC 9110 token, up to 16 chars |
| `content_type` | string \| null | Default `null` = keep the captured type |
| `header_allowlist` | string[] | Default `[]`; header names, case-insensitive, up to 50 |
| `append_path` | boolean | Default false; append captured sub-path and query to `target_url` |
| `created_at` | timestamp | Read-only |

**ForwardRuleInput**: the writable fields above. On `POST` absent fields take their defaults; on `PATCH`
absent fields are unchanged and `null` resets `method` or `content_type`.

**ForwardRequest** (manual resend body):

| Field | Type | Notes |
|---|---|---|
| `rule_id` | uuid \| null | Optional. When set, the rule's settings are used and every other field must be absent |
| `target_url` | string \| null | Required unless `rule_id` is set |
| `method` | string \| null | Absent or `null` = captured method |
| `content_type` | string \| null | Absent or `null` = captured type |
| `header_allowlist` | string[] \| null | Absent or `null` = `[]` |

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
| `form` | `{fields: {name, value}[], files: FormFile[]}` \| null | Only for `application/x-www-form-urlencoded` and `multipart/form-data`; `null` otherwise or when parsing failed |
| `body` | Body | First 1 MiB; `truncated: true` when `body_size` is larger, fetch the whole body from `/body` |
| `forwards` | ForwardJob[] | Newest first |

FormFile: `{name: string, filename: string | null, content_type: string | null, size: integer}`; `filename` and
`content_type` are `null` when the part did not declare them. File content stays in the raw body.

### 4.5 ForwardJob

| Field | Type |
|---|---|
| `id` | uuid |
| `kind` | `"rule"` \| `"manual"` |
| `rule_id` | uuid \| null |
| `target_url`, `method` | string |
| `state` | `"pending"` \| `"running"` \| `"succeeded"` \| `"failed"` |
| `attempts` | ForwardAttempt[], oldest first |
| `created_at` | timestamp |
| `finished_at` | timestamp \| null |

ForwardAttempt: `{attempt: integer, started_at: timestamp, duration_ms: integer, status: integer | null,
error: string | null, final_url: string | null, response_content_type: string | null, response_body: Body | null}`.
`status` is `null` when no response arrived; `response_body` holds the first 4 KiB, `truncated: true` when the
response was longer, `null` when no response arrived.

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
| `DELETE /api/v1/endpoints/{id}/members/{user_id}` | owner, `sso` | – | 204; 404 `member_not_found` |
| `GET /api/v1/endpoints/{id}/forward-rules` | viewer | – | 200 `{items: ForwardRule[]}` |
| `POST /api/v1/endpoints/{id}/forward-rules` | editor | ForwardRuleInput | 201 ForwardRule |
| `PATCH /api/v1/endpoints/{id}/forward-rules/{rule_id}` | editor | ForwardRuleInput | 200 ForwardRule |
| `DELETE /api/v1/endpoints/{id}/forward-rules/{rule_id}` | editor | – | 204 |
| `GET /api/v1/endpoints/{id}/captures` | viewer | query below | 200 `{items: CaptureSummary[], next_cursor, total}` |
| `GET /api/v1/endpoints/{id}/captures/latest` | viewer | – | 200 Capture, 404 `capture_not_found` if empty |
| `GET /api/v1/endpoints/{id}/captures/{capture_id}` | viewer | – | 200 Capture |
| `GET /api/v1/endpoints/{id}/captures/{capture_id}/body` | viewer | `?view=text&download=1` | 200 raw bytes, §5.2 |
| `DELETE /api/v1/endpoints/{id}/captures/{capture_id}` | editor | – | 204 |
| `DELETE /api/v1/endpoints/{id}/captures` | editor | – | 204 |
| `POST /api/v1/endpoints/{id}/captures/{capture_id}/forward` | editor | ForwardRequest | 202 ForwardJob (`state: "pending"`) |
| `GET /api/v1/endpoints/{id}/stream` | viewer | `Last-Event-ID` header or `?last_event_id=` | 200 `text/event-stream`, §6 |

Reading an endpoint, its captures or its stream extends its lifetime (E3) when `last_active_at` is older
than 60 s; captures always extend it. `PUT .../members` with an email
nobody has signed in with answers 404 `user_not_found`; the owner cannot add themself (422). Every rule change
also emits `endpoint_updated` on the stream.

**Instance**

```json
{
  "version": "1.0.0",
  "auth_mode": "sso",
  "user": { "id": "0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21", "email": "dev@example.com", "name": "Dev" },
  "public_url": "https://webhooks.globalart.hu",
  "auth": { "provider_name": "GlobalArt SSO" },
  "limits": {
    "max_captures": 500,
    "max_body_bytes": 10485760,
    "retention_seconds": 604800,
    "max_delay_ms": 10000,
    "max_forward_rules": 5,
    "max_response_body_bytes": 65536,
    "max_response_headers": 20
  },
  "features": {
    "ring_mode_allowed": true,
    "forwarding": true,
    "private_forward_targets_allowed": false
  }
}
```

| Field | Type | Notes |
|---|---|---|
| `version` | string | Build version of the server binary |
| `auth_mode` | `"open"` \| `"sso"` \| `"mixed"` | Access mode |
| `user` | User \| null | Signed-in user; `null` when anonymous or in `open`. Never answers 401 |
| `public_url` | string | Base of capture URLs |
| `auth.sign_in_available` | boolean | `true` once OIDC discovery has succeeded (`sso`, `mixed`); `false` in `open` and while the identity provider is unreachable |
| `auth.provider_name` | string \| null | Sign-in button label from `WEBHOOKS_LIVE_OIDC_PROVIDER_NAME`; `null` when not configured or in `open`, the UI then uses a neutral label |
| `limits.*` | integer | Instance limits; `max_body_bytes` is the capture body limit |
| `features.ring_mode_allowed` | boolean | Whether `overflow: ring` is accepted |
| `features.forwarding` | boolean | Forward rules and resend available |
| `features.private_forward_targets_allowed` | boolean | True when the operator's forward allow list (`WEBHOOKS_LIVE_FORWARD_ALLOW_CIDRS`) is non-empty; the ranges themselves are not exposed |

### 5.1 Capture list query

| Param | Type | Meaning |
|---|---|---|
| `order` | `desc` \| `asc` | By `seq`; default `desc` |
| `limit` | integer | 1–200, default 50 |
| `cursor` | string | From `next_cursor` |
| `method` | string | Comma list, e.g. `POST,PUT` |
| `status` | string | Comma list of override classes `1xx`…`5xx`, or `none` |
| `q` | string | Case-insensitive substring in path, query, headers, first 256 KiB of body; 1–200 chars |
| `after_seq` | integer | Only captures with `seq` greater than this; combine with `order=asc` |
| `since`, `until` | timestamp | `received_at` range, inclusive start, exclusive end; for the user's time filter |

`total` is the number of captures matching the filters, ignoring the cursor and `after_seq`.

Reconnect catch-up uses `after_seq`, never `since`: after a stream reconnects (`ready` received) or on
`resync`, the client lists `after_seq=<highest seq it holds>&order=asc`, follows `next_cursor` until `null`,
and merges by `id`. `seq` is assigned in commit order per endpoint, so nothing is skipped; timestamps come
from replica clocks and can be out of order. Deletions missed while offline are not replayed: when
`ready.capture_count` differs from the client's count after the merge, it refetches the first page.

### 5.3 Forward rules

Rules are edited one at a time so each error lands on its own form. On `POST` and on `PATCH` that changes
`target_url`, the server parses the URL and resolves its host. Failure answers 422:

```json
{
  "error": {
    "code": "validation_failed",
    "message": "target resolves only to refused addresses",
    "fields": [{ "field": "target_url", "code": "target_refused", "message": "10.0.0.5 is a private address" }]
  }
}
```

| Field code on `target_url` | When |
|---|---|
| `invalid_format` | Not an absolute `http`/`https` URL, or longer than 2048 chars |
| `target_refused` | Literal or every resolved address is in a refused range |
| `target_unresolvable` | Host does not resolve |

The same codes apply to `target_url` in ForwardRequest. A sixth rule answers 422 `forward_rule_limit`.
Passing the save-time check does not guarantee delivery: every attempt and redirect is checked again.

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
| `ready` | – | `{"endpoint_id": string, "last_seq": integer, "capture_count": integer}` | Always the first event of every connection, before any replay; start catch-up (§5.1) |
| `capture` | `seq` | CaptureSummary | Prepend to list; fetch detail on demand |
| `capture_deleted` | – | `{"capture_id": uuid, "seq": integer}` | Remove from list |
| `captures_cleared` | – | `{}` | Empty the list |
| `endpoint_updated` | – | Endpoint | Replace settings |
| `endpoint_deleted` | – | `{}` | Show gone state, stop |
| `forward_result` | – | `{"capture_id": uuid, "job": ForwardJob}` | Update forward state |
| `resync` | – | `{"reason": "lagged" \| "gap" \| "listener_reconnect"}` | Re-list from the top |
| `bye` | – | `{}` | Server shutting down; let `EventSource` reconnect. A connection opened during drain gets 200 and an immediate `bye` |

Resume: `EventSource` sends `Last-Event-ID` (the last `capture` `seq`) on reconnect. If both `?last_event_id`
and the header are present the larger wins; a malformed value is ignored. The server replays
missed `capture` events in ascending `seq` before live ones, up to 100; beyond that it sends `resync`. A page
that already holds a list opens the stream with `?last_event_id=<highest seq it has>` to close the gap between
its list request and the stream. Replayed events and the `after_seq` catch-up list (§5.1) may overlap; merge
by `id`. Live delivery is best-effort: `resync` means "list again from `after_seq`".

## 7. Auth routes (`sso` mode)

| Method, path | Behaviour |
|---|---|
| `GET /auth/login?return_to=/e/{id}` | 302 to the issuer. `return_to` must start with `/` and not `//`; else `/`. Sets a short-lived cookie `__Host-whl_login_<12 characters of the state>`, so several sign-ins can be in flight in one browser. 503 `auth_unavailable` or `login_busy`. |
| `GET /auth/callback?code&state` | Validates, creates the session, sets `__Host-whl_session`, 302 to `return_to`. Failure: 302 to `/?login_error=<code>` and only the cookie of that attempt is cleared. Codes include `invalid_state`, `provider_denied`, `invalid_request`, `exchange_failed`, `invalid_token`, `missing_email`, `email_not_verified`, `email_verified_invalid` (a non-boolean claim), `unavailable`. |
| `POST /auth/logout` | Origin-checked; deletes the session, clears the cookie, 204. |

In `open` mode these answer 404. `/auth/login` and `/auth/callback` share the per-IP management limiter. A request carrying the same cookie name twice, or a cookie name that only decodes to the session or login name, is treated as carrying no such cookie.

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

curl -s 'https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures?after_seq=41&order=asc'

curl -s https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures/latest

curl -s -o body.bin 'https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures/0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b/body?download=1'

curl -s -X POST https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/forward-rules \
  -H 'Content-Type: application/json' \
  -d '{"target_url":"https://api.example.com/hooks","header_allowlist":["X-Signature"],"append_path":true}'

curl -s -X POST https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures/0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b/forward \
  -H 'Content-Type: application/json' -d '{"target_url":"https://staging.example.com/hooks"}'

curl -N -H 'Last-Event-ID: 41' https://webhooks.globalart.hu/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/stream
```

Stream output:

```
retry: 2000

event: ready
data: {"endpoint_id":"7k2m9q4xw8c1d5f3h6j0n2p4","last_seq":42,"capture_count":42}

id: 42
event: capture
data: {"id":"0193a1c2-9d10-7a3e-8b44-1c2d3e4f5a6b","seq":42,"endpoint_id":"7k2m9q4xw8c1d5f3h6j0n2p4","received_at":"2026-11-21T10:16:02.511Z","method":"POST","path":"/404/orders","query_string":"x=1","client_ip":"203.0.113.7","content_type":"application/json","body_size":12,"status_override":404,"response_status":404,"forward_state":null}

: ping
```

In `sso` mode the same calls need the session cookie, and mutating ones an `Origin` header; machine access
in `sso` mode is not part of the first release.

## 9. Parity additions: conventions

Sections 9–24 add the parity plan (product-spec §13–§28, architecture §16–§36). §1–§8 stay valid except where
a row below extends them.

| Topic | Rule |
|---|---|
| Mode `mixed` | Anonymous callers work as in `open` on anonymous endpoints; owned endpoints follow `sso` rules. Person-only routes answer 403 `sign_in_required` to anonymous callers. |
| Tokens | `Authorization: Bearer whl_...` on every `/api/v1` route, streams included. With a Bearer header the cookie is ignored and the origin check is skipped. Token, member and ownership routes refuse tokens with 403 `session_required`. |
| Grants | Anonymous endpoints with a view password need `X-Webhooks-Live-Grant: <grant>` on every read; stream routes also accept `?grant=`. Missing or expired: 401 `grant_required`. |
| CORS | `/api/v1/*` answers preflights 204 and every response with `Access-Control-Allow-Origin: *`, never with credentials (architecture §19). |
| Body limits | `POST /api/v1/send`: 1.5 MiB. `POST .../import`: the tier's import size, `application/json` or `application/x-ndjson`. Others: §1. |
| Templates in JSON | Template fields are plain strings in the §21 syntax of architecture; field errors carry `line` and `column`. |

| Status | `code` | When |
|---|---|---|
| 401 | `invalid_token` | Bearer token unknown, expired or revoked |
| 401 | `grant_required` | View-protected endpoint without a valid grant |
| 401 | `wrong_password` | Unlock with a wrong password |
| 403 | `sign_in_required` | Person-only feature, anonymous caller |
| 403 | `session_required` | Route refuses tokens |
| 403 | `claim_refused` | Claim without the proof of architecture §17 |
| 409 | `claim_required` | `mixed`: member routes on an endpoint that has no owner yet |
| 403 | `insufficient_scope` | Token scope or endpoint limit too narrow |
| 403 | `tier_not_allowed` | Feature not in the caller's tier (`error.limit.name` names it) |
| 404 | `rule_not_found`, `secret_not_found`, `var_not_found`, `channel_not_found`, `schedule_not_found`, `template_not_found`, `folder_not_found`, `token_not_found`, `alias_not_found`, `part_not_found`, `rule_run_not_found`, `email_address_not_found`, `subscription_not_found`, `sent_request_not_found` | Unknown id in its scope |
| 409 | `already_owned` | Claim of an owned endpoint |
| 409 | `alias_taken` | Alias in use or in quarantine |
| 409 | `name_taken` | Folder, template or secret name already used where unique |
| 409 | `secret_in_use` | Secret still referenced; `error.references` lists rule, channel, schedule or verifier ids |
| 422 | `limit_reached` | Count limit hit (rules, aliases, tokens, schedules, folders, never-expire); `error.limit` = `{name, max}` |
| 422 | `export_too_large` | Selection above the tier export size |
| 429 | `quota_exceeded` | Durable quota hit; `error.limit` = `{name, window, max, resets_at}`, `Retry-After` set |
| 503 | `feature_disabled` | Operator switched the feature off (secrets keyring, mail, DNS, push, email) |

| Field code | Raised for |
|---|---|
| `template_invalid` | Template does not parse; `line`, `column` added |
| `script_invalid` | Script does not compile; `line`, `column` added |
| `regex_invalid`, `json_path_invalid`, `cron_invalid`, `timezone_invalid` | The named syntax does not parse |
| `secret_unknown` | `secret.<NAME>` or a secret field names a missing secret |
| `secret_not_allowed` | Secret referenced in a response step or response template |
| `step_unknown` | `step.<name>` refers to no earlier step |
| `tier_not_allowed` | Value allowed only in a higher tier (for example an HTTP step before the response) |
| `reserved` | Reserved alias |
| `not_verified` | Email channel recipient not verified |
| `host_not_allowed` | Slack, Discord or push URL outside its fixed host pattern |
| `interval_too_short` | Schedule fires more often than the tier allows |

```json
{
  "error": {
    "code": "quota_exceeded",
    "message": "outbound requests per hour exhausted",
    "limit": { "name": "outbound", "window": "hour", "max": 200, "resets_at": "2026-11-21T11:00:00.000Z" }
  }
}
```

## 10. Identity, tiers, usage

`GET /api/v1/instance` gains:

| Field | Type | Notes |
|---|---|---|
| `auth_mode` | `"open"` \| `"sso"` \| `"mixed"` | |
| `tier` | string | Caller's tier: `anonymous`, `member`, or an operator tier |
| `limits` | object | Values of the caller's tier; adds `max_captures_settable`, `max_retention_seconds`, `never_expire_endpoints`, `rules_per_endpoint`, `steps_per_rule`, `sync_http_steps`, `aliases`, `tokens`, `schedules`, `schedule_min_interval_seconds`, `export_max_bytes`, `import_max_bytes` |
| `features.secrets`, `features.rules`, `features.scripts`, `features.schedules`, `features.api_cors` | boolean | |
| `features.notifications` | string[] | Channel kinds available to the caller |
| `features.mail_domain` | string \| null | Set when email capture is on |
| `features.dns_zone` | string \| null | Set when DNS capture is on |
| `features.subdomain_host` | string \| null | Set when subdomain capture is on |
| `features.push_public_key` | string \| null | VAPID public key, base64url |

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/usage` | any | – | 200 Usage |
| `POST /api/v1/endpoints/{id}/claim` | signed in, `mixed` | – | 200 Endpoint with `my_role: "owner"`; 403 `sign_in_required`, 403 `claim_refused` (caller outside the creating network, endpoint older than 24 hours, or no recorded network), 409 `already_owned` (caller is the owner or a member), 410 `endpoint_gone` (including an endpoint owned by someone else) |

Usage: `{tier: string, items: [{name: string, window: "hour" | "day" | "total", used: integer, max: integer, resets_at: timestamp | null}]}`.
Names: `endpoints`, `storage_bytes`, `outbound`, `notifications`, `email_notifications`, `exports`, `imports`,
`composer`, `schedules`, `aliases`, `tokens`, `never_expire`.

## 11. API tokens

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/tokens` | session | – | 200 `{items: ApiToken[]}` |
| `POST /api/v1/tokens` | session | ApiTokenInput | 201 CreatedApiToken |
| `DELETE /api/v1/tokens/{token_id}` | session | – | 204 (revokes) |

| ApiToken field | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `name` | string | 1–64 chars |
| `prefix` | string | First 12 characters of the token |
| `scopes` | string[] | Subset of `read`, `endpoints:write`, `captures:write`, `rules:write`, `send` |
| `endpoint_ids` | string[] \| null | Limit to these endpoints, at most 50 |
| `created_at`, `expires_at` | timestamp | |
| `last_used_at`, `revoked_at` | timestamp \| null | |

ApiTokenInput: `{name, scopes, endpoint_ids?, expires_in_days}` with `expires_in_days` 1–365 (default 90).
CreatedApiToken = ApiToken plus `token` (the only time it is returned).

## 12. Endpoint additions

Endpoint (§4.1) gains:

| Field | Type | Notes |
|---|---|---|
| `tags` | string[] | At most 10, each `^[a-z0-9][a-z0-9-]{0,31}$` |
| `retention` | `{mode: "sliding" \| "never", seconds: integer \| null}` | `seconds` 3600 to tier max, `null` = instance default; `never` for signed-in owners within the tier count |
| `max_captures` | integer | Now writable by the owner, 1 to tier max |
| `capture_ttl_seconds` | integer \| null | At least 60 |
| `sender_allow_cidrs` | string[] | At most 20 CIDRs |
| `capture_auth` | CaptureAuth \| null | Output: `{kind: "basic", username}` or `{kind: "header", name}`; secrets never returned |
| `view_protected` | boolean | Read-only; true when a view password is set |
| `response.templated` | boolean | Default false |
| `verifier` | Verifier \| null | |
| `email` | `{enabled: boolean, address: string \| null}` | `address` = `<id>@<mail domain>` when the instance has one |
| `dns` | `{enabled: boolean, name: string \| null}` | `name` = `<id>.<zone>` |
| `aliases` | string[] | Read-only here; §12 alias routes |
| `subdomain_url` | string \| null | Read-only |
| `rule_count` | integer | Read-only |

EndpointInput adds `tags`, `retention`, `max_captures`, `capture_ttl_seconds`, `sender_allow_cidrs`, `capture_auth`,
`view_password` (string 8–128 chars, write-only, `null` removes; anonymous endpoints only, else `not_allowed`),
`response.templated`, `verifier`, `email.enabled`, `dns.enabled`. On create it also accepts `template_id`.

CaptureAuth input: `{"kind": "basic", "username": string, "password": string}` or
`{"kind": "header", "name": string, "value": string}`; `null` removes. Values 1–256 bytes, visible ASCII.

Verifier: `{"scheme": "hmac" | "github" | "stripe" | "slack" | "shopify" | "standard_webhooks", "secret_name": string,
"header": string | null, "algorithm": "sha1" | "sha256" | "sha512" | null, "encoding": "hex" | "base64" | null,
"prefix": string | null, "timestamp_header": string | null}`. Presets fill the nullable fields; `hmac` requires
`header`, `algorithm`, `encoding`.

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints` | signed in | adds `?tag&folder&q` (title, id, alias) | as §5 |
| `POST /api/v1/endpoints/{id}/unlock` | anyone with the id | `{password}` | 200 `{grant: string, expires_at}`; 401 `wrong_password`; 429 |
| `POST /api/v1/endpoints/{id}/clone` | viewer on source (`sso`/`mixed`: signed in) | `{title?, include?: ("response" \| "forward_rules" \| "rules" \| "channels" \| "schedules" \| "secrets")[]}` (default all but secrets) | 201 Endpoint |
| `GET /api/v1/endpoints/{id}/aliases` | viewer | – | 200 `{items: Alias[]}` |
| `PUT /api/v1/endpoints/{id}/aliases/{alias}` | owner, signed in | – | 200 Alias; 409 `alias_taken`; 422 `reserved` |
| `DELETE /api/v1/endpoints/{id}/aliases/{alias}` | owner | – | 204 |
| `GET /api/v1/endpoints/{id}/stats` | viewer | `?bucket=hour\|day` | 200 Stats |
| `PATCH /api/v1/endpoints/{id}/captures/{capture_id}` | editor | `{note?: string \| null, starred?: boolean}` | 200 CaptureSummary |
| `GET /api/v1/folders` | signed in | – | 200 `{items: Folder[]}` |
| `POST /api/v1/folders` | signed in | `{name}` | 201 Folder |
| `PATCH /api/v1/folders/{folder_id}` | signed in | `{name?, position?}` | 200 Folder |
| `DELETE /api/v1/folders/{folder_id}` | signed in | – | 204 |
| `PUT /api/v1/folders/{folder_id}/endpoints/{id}` | signed in, viewer on the endpoint | – | 204 |
| `DELETE /api/v1/folders/{folder_id}/endpoints/{id}` | signed in | – | 204 |
| `GET /api/v1/templates` | signed in | – | 200 `{items: Template[]}` |
| `POST /api/v1/templates` | signed in | `{name, from_endpoint_id}` | 201 Template |
| `GET /api/v1/templates/{template_id}` | signed in | – | 200 Template |
| `PATCH /api/v1/templates/{template_id}` | signed in | `{name}` | 200 Template |
| `DELETE /api/v1/templates/{template_id}` | signed in | – | 204 |

Alias: `{alias, url, created_at}` with `url` = `<public url>/a/<alias>`. Folder: `{id, name, position,
endpoint_ids: string[], created_at}`. Template: `{id, name, created_at, updated_at, config}` where `config` is the
configuration document of architecture §24. Stats: `{bucket, from, to, series: [{at: timestamp, total, by_method:
{<method>: n}, by_kind: {<kind>: n}, by_status: {"2xx": n, ...}}]}`.

## 13. Secrets

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/secrets` | editor | – | 200 `{items: [{name, created_at, updated_at}]}` |
| `PUT /api/v1/endpoints/{id}/secrets/{name}` | editor | `{value: string}` (1–4096 bytes) | 200 `{name, created_at, updated_at}` |
| `DELETE /api/v1/endpoints/{id}/secrets/{name}` | editor | – | 204; 409 `secret_in_use` with the referencing rule, channel, schedule or verifier ids |

Values are never returned. Name: `^[A-Z][A-Z0-9_]{0,63}$`. At most 20.

## 14. Persistent variables

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/vars` | viewer | – | 200 `{items: [{name, value, updated_at}]}` |
| `PUT /api/v1/endpoints/{id}/vars/{name}` | editor | `{value: any JSON}` (at most 4 KiB serialized) | 200 Var |
| `DELETE /api/v1/endpoints/{id}/vars/{name}` | editor | – | 204 |

Name `^[a-z_][a-z0-9_]{0,63}$`, at most 50. Counters are variables holding an integer.

## 15. Rules

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/rules` | viewer | – | 200 `{items: Rule[], version: integer}` |
| `POST /api/v1/endpoints/{id}/rules` | editor | RuleInput | 201 Rule (appended last) |
| `GET /api/v1/endpoints/{id}/rules/{rule_id}` | viewer | – | 200 Rule |
| `PATCH /api/v1/endpoints/{id}/rules/{rule_id}` | editor | RuleInput (partial) | 200 Rule |
| `DELETE /api/v1/endpoints/{id}/rules/{rule_id}` | editor | – | 204 |
| `PUT /api/v1/endpoints/{id}/rules/order` | editor | `{ids: uuid[]}` (every rule exactly once) | 200 `{items: Rule[], version}` |
| `POST /api/v1/endpoints/{id}/rules/dry-run` | editor | `{capture_id, rules?: RuleInput[], execute_http?: boolean}` | 200 RuleRun (not stored; `rules` tests unsaved drafts) |
| `GET /api/v1/endpoints/{id}/captures/{capture_id}/rule-runs` | viewer | – | 200 `{items: RuleRun[]}` newest first |
| `POST /api/v1/endpoints/{id}/captures/{capture_id}/rule-runs` | editor | – | 202 RuleRun (`trigger: "rerun"`, `state: "queued"`) |

Every change emits `endpoint_updated` and bumps `version`.

| Rule field | Type | Notes |
|---|---|---|
| `id` | uuid | Read-only |
| `name` | string | 1–100 chars |
| `enabled` | boolean | Default true |
| `condition` | Condition | Default `{}` (always) |
| `steps` | Step[] | 1 to the tier's steps per rule |
| `position` | integer | Read-only |
| `created_at`, `updated_at` | timestamp | Read-only |

Condition: `{}`, `{"all": Condition[]}`, `{"any": Condition[]}`, `{"not": Condition}`, or a leaf
`{"value": "<variable path>", "op": "eq" | "ne" | "contains" | "starts_with" | "ends_with" | "matches" | "exists" | "gt" | "lt" | "in" | "in_cidr", "arg": string | number | string[] | null}`.
`matches` takes a regex, `in` and `in_cidr` a list, `exists` no `arg`. At most 4 levels and 32 leaves.

Every step has `kind`, optional `name` (`^[a-z][a-z0-9_]{0,31}$`, unique per endpoint, required when later steps
read its result) and `continue_on_error` (default false).

| `kind` | Fields |
|---|---|
| `respond` | `status` (template), `content_type` (template \| null), `headers` (`{name, value}[]`, values templated, at most 20), `body` (template, at most 64 KiB) |
| `set_header` | `name`, `value` (template) |
| `remove_header` | `name` |
| `delay` | `ms` (0–10000) |
| `stop` | – |
| `extract` | `from` (variable path), one of `json_path` or `regex` (with `group`, default 1), `into` (run variable) |
| `set_var` | `into`, `value` (template) |
| `transform` | `into` (run variable or `"payload"`), `template` |
| `set_store` | `name` (persistent variable), `value` (template) |
| `increment` | `counter` (persistent variable), `by` (integer, default 1) |
| `script` | `source` (Rhai, at most 16 KiB) |
| `http` | `method`, `url` (template), `headers` (`{name, value}[]`, templated), `body` (template \| null), `timeout_ms` (at most 15000, default 10000), `follow_redirects` (default true) |
| `forward` | `forward_rule_id`, or `target_url` with optional `method`, `content_type`, `header_allowlist`; `body_from`: `"capture"` (default) or `"payload"` |
| `notify` | `channel_id`, `message` (template \| null: channel default) |
| `store_s3` | `bucket_url`, `prefix`, `region`, `access_key_secret`, `secret_key_secret`, `content`: `"capture_json"` or `"body"` |

RuleRun:

| Field | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `capture_id` | uuid | |
| `trigger` | `"capture"` \| `"rerun"` \| `"dry_run"` | |
| `state` | `"sync"` \| `"queued"` \| `"running"` \| `"succeeded"` \| `"failed"` | |
| `rules_version` | integer | |
| `response_status` | integer \| null | Status the rules produced |
| `error` | string \| null | `template`, `budget`, `script`, `http`, `quota_exceeded`, `overloaded`, `hop_limit`, `internal`, then `: ` and detail |
| `entries` | RunEntry[] | In execution order |
| `created_at`, `finished_at` | timestamp, timestamp \| null | |

RunEntry: `{rule_id, rule_name, matched: boolean}` for a rule, or `{rule_id, step: integer, name: string | null,
kind, outcome: "ok" | "failed" | "skipped" | "deferred" | "no_op", duration_ms, detail: string | null, output: any | null}`
for a step. `output` holds at most 4 KiB: the HTTP result `{status, headers, body: Body, duration_ms}`, extracted
values, or script output; secrets appear as `[secret:NAME]`.

## 16. Notifications

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/notifications` | viewer | – | 200 `{items: Channel[]}` |
| `POST /api/v1/endpoints/{id}/notifications` | editor | ChannelInput | 201 Channel |
| `PATCH /api/v1/endpoints/{id}/notifications/{channel_id}` | editor | ChannelInput (partial) | 200 Channel |
| `DELETE /api/v1/endpoints/{id}/notifications/{channel_id}` | editor | – | 204 |
| `POST /api/v1/endpoints/{id}/notifications/{channel_id}/test` | editor | – | 202 Delivery |
| `POST /api/v1/notifications/disable` | none | `{token}`; also `?token=` for one-click `List-Unsubscribe-Post` | 204 (channel disabled) |
| `GET /api/v1/email-addresses` | signed in | – | 200 `{items: EmailAddress[]}` |
| `POST /api/v1/email-addresses` | signed in | `{address}` | 201 EmailAddress (verification sent) |
| `DELETE /api/v1/email-addresses/{address_id}` | signed in | – | 204 |
| `POST /api/v1/email-addresses/verify` | none | `{token}` | 200 EmailAddress |
| `POST /api/v1/push/subscriptions` | signed in | `{endpoint, keys: {p256dh, auth}}` (browser `PushSubscription` JSON) | 201 `{id, created_at}` |
| `DELETE /api/v1/push/subscriptions/{subscription_id}` | signed in | – | 204 |

| Channel field | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `kind` | `"webhook"` \| `"slack"` \| `"discord"` \| `"telegram"` \| `"email"` \| `"push"` | |
| `name` | string | 1–64 chars |
| `enabled` | boolean | |
| `trigger` | `"capture"` \| `"rule"` | |
| `condition` | Condition \| null | `capture` trigger only |
| `config` | object | Per kind, below |
| `template` | string \| null | Message template; `null` = default |
| `per_hour` | integer | 1 to the tier maximum |
| `created_at` | timestamp | |

| `kind` | `config` |
|---|---|
| `webhook` | `{url}`, checked like a forward target |
| `slack` | `{url}` matching the Slack incoming-webhook host pattern; stored as a secret, returned masked (`https://hooks.slack.com/services/...`) |
| `discord` | `{url}` matching the Discord webhook pattern; stored and returned like Slack |
| `telegram` | `{token_secret, chat_id}`; `token_secret` names a secret |
| `email` | `{address_id}`, a verified address of the caller |
| `push` | `{}`: the creator's subscriptions |

Delivery: `{id, channel_id, kind: "single" | "summary" | "test", state: "pending" | "running" | "succeeded" | "failed" | "suppressed", attempts, status: integer | null, error: string | null, created_at, finished_at}`.
EmailAddress: `{id, address, verified: boolean, created_at}`.

## 17. Composer

| Method, path | Role | Request | Success |
|---|---|---|---|
| `POST /api/v1/send` | any; `send` scope | SendRequest | 200 SendResult |
| `GET /api/v1/send/history` | signed in, or `?endpoint_id=` with viewer | `?cursor&limit` | 200 `{items: SentRequest[], next_cursor}` |
| `DELETE /api/v1/send/history/{sent_id}` | owner of the entry | – | 204 |

SendRequest: `{method, url, headers: {name, value}[], body: Body | null, timeout_ms?: integer (≤ 15000), follow_redirects?: boolean (default false), endpoint_id?: string, source_capture_id?: uuid}`.
`url` gets the forward-target field codes (§5.3). SendResult: `{id, status: integer | null, headers: {name, value}[],
body: Body | null (first 1 MiB), duration_ms, final_url: string | null, error: string | null, capture_id: uuid | null}`;
`capture_id` is set when the target was this instance's own capture URL. SentRequest = SendResult plus
`request` (the SendRequest without `body` data beyond 64 KiB) and `created_at`.

## 18. Schedules

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/schedules` | viewer | – | 200 `{items: Schedule[]}` |
| `POST /api/v1/endpoints/{id}/schedules` | owner, signed in | ScheduleInput | 201 Schedule |
| `PATCH /api/v1/endpoints/{id}/schedules/{schedule_id}` | editor | ScheduleInput (partial); `enabled: true` clears a pause | 200 Schedule |
| `DELETE /api/v1/endpoints/{id}/schedules/{schedule_id}` | editor | – | 204 |
| `POST /api/v1/endpoints/{id}/schedules/{schedule_id}/run` | editor | – | 202 `{schedule_id, queued_at}` |

| Schedule field | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `name` | string | 1–100 chars |
| `cron` | string | 5 fields; `cron_invalid`, `interval_too_short` |
| `timezone` | string | IANA name, default `UTC` |
| `request` | `{method, url, headers: {name, value}[], body: string \| null}` | Templates; secrets allowed in `url` and header values |
| `enabled` | boolean | |
| `paused_reason` | string \| null | Read-only |
| `next_run_at`, `last_run_at` | timestamp \| null | Read-only |
| `consecutive_failures` | integer | Read-only |
| `created_at` | timestamp | Read-only |

## 19. Export and import

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/export` | viewer | `?format=json\|ndjson\|csv\|zip` plus §5.1 filters and `kind` | 200 stream |
| `POST /api/v1/endpoints/{id}/export` | viewer | `{format, capture_ids: uuid[]}` (at most 500) | 200 stream |
| `POST /api/v1/endpoints/{id}/import` | owner, signed in | JSON or NDJSON export | 200 `{imported: integer, skipped: integer, reason: "capacity" \| null}` |

| Format | `Content-Type` | File name |
|---|---|---|
| `json` | `application/json` | `webhooks-live-<id>-<yyyymmddHHMMSS>.json` |
| `ndjson` | `application/x-ndjson` | `.ndjson` |
| `csv` | `text/csv; charset=utf-8` | `.csv` |
| `zip` | `application/zip` | `.zip` |

Every export answers with `Content-Disposition: attachment`, `X-Content-Type-Options: nosniff`, `Cache-Control:
no-store`, chunked transfer. The JSON document and NDJSON header carry `format: "webhooks-live-export"`,
`version: 1`, `exported_at`, `endpoint` (EndpointInput fields plus `id`, without secrets). Import errors answer
422 `validation_failed` with `field` = `captures.<index>.<path>` or `line.<n>` for NDJSON.

## 20. Combined inbox

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/inbox/captures` | viewer on each endpoint | `?endpoints=<ids, comma>` or `?folder=<id>` (signed in; none = all own and shared, at most 50), §5.1 filters except `after_seq`, `kind`, `cursor`, `limit` | 200 `{items: CaptureSummary[], next_cursor}` newest first |
| `GET /api/v1/inbox/stream` | viewer on each endpoint | same selection; `Last-Event-ID` or `?last_event_id` | 200 `text/event-stream` |

The inbox stream sends §6 events for every endpoint in the set (payloads carry `endpoint_id`) except
`endpoint_updated`. Event ids are opaque; resume works as §6 with at most 100 replayed events.

## 21. Capture additions

CaptureSummary (§4.3) gains `kind` (`"http"` \| `"email"` \| `"dns"` \| `"schedule"`), `starred` (boolean),
`has_note` (boolean), `verification` (`"valid"` \| `"invalid"` \| `"missing"` \| null), `rule_state`
(`"running"` \| `"succeeded"` \| `"failed"` \| null), `imported` (boolean), and `subject` (string \| null:
email subject, DNS name, or schedule name, up to 200 chars). The list query (§5.1) gains `kind` (comma list) and
`starred` (boolean).

Capture (§4.4) gains `note`, `meta` (per kind, below), `rule_runs` (RuleRun[], newest first, at most 10) and
`notifications` (Delivery[]).

| `kind` | `meta` |
|---|---|
| `http` | `{hop: integer}` |
| `email` | `{envelope_from, envelope_to: string[], helo, tls: boolean, tag: string \| null, subject, from, to: string[], cc: string[], date: timestamp \| null, message_id: string \| null, parts: Part[]}` |
| `dns` | `{name, type, class, transport: "udp" \| "tcp", dns_id, flags: {rd, cd, do}, edns_udp_size: integer \| null, client_subnet: string \| null}` |
| `schedule` | `{schedule_id, schedule_name, request: {method, url}, duration_ms, final_url, error, truncated}` |

Part: `{index, content_type, filename: string | null, size, disposition: "inline" | "attachment" | null, is_text: boolean}`.

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/captures/{capture_id}/parts/{index}` | viewer | `?view=text&download=1` | 200 decoded part with the §5.2 headers; `filename` from the part, sanitised |

Capture path (§3) additions:

| Case | Status | `X-Webhooks-Live-Error` |
|---|---|---|
| Sender outside the allow list | 403 `Forbidden.` | `sender_refused` |
| Capture auth missing or wrong | 401 `Unauthorized.` (no `WWW-Authenticate`) | `unauthorized` |
| Owner over storage quota | 507 | `quota_exceeded` |
| Unknown alias | 410 | `endpoint_gone` |

| Response header | When | Value |
|---|---|---|
| `X-Webhooks-Live-Rule-Error` | Rules failed before the response | `template`, `budget`, `script`, `http`, `quota`, `overloaded`, `internal` |
| Rule-set headers | Rules ran | From `respond` and `set_header`, subject to the §9 header rules of architecture |

`/a/{alias}[/{rest}]` and `https://{id}.<subdomain host>/{rest}` behave exactly as `/{id}[/{rest}]`.

## 22. Stream additions

| Event | `id` | `data` | Client action |
|---|---|---|---|
| `capture_updated` | – | `{capture_id, seq, note_changed: boolean, starred, response_status, rule_state, verification}` | Patch the list row |
| `rule_result` | – | `{capture_id, run: RuleRun}` | Update the rule panel |
| `notification_result` | – | `{capture_id, delivery: Delivery}` | Update delivery state |
| `captures_imported` | – | `{imported: integer}` | Re-list from the top |

## 23. OpenAPI

`GET /api/v1/openapi.json`: OpenAPI 3.1 document of every `/api/v1` route, `ETag` = build version, public in all
modes. The reference page is a UI route (ui-design.md).

## 24. Examples

```sh
curl -s -X POST https://webhooks.live/api/v1/tokens \
  -H 'Content-Type: application/json' -H 'Origin: https://webhooks.live' -b '__Host-whl_session=...' \
  -d '{"name":"laptop cli","scopes":["read"],"expires_in_days":30}'

curl -s -H 'Authorization: Bearer whl_q3k...' https://webhooks.live/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4/captures?kind=email

curl -s -X PATCH https://webhooks.live/api/v1/endpoints/7k2m9q4xw8c1d5f3h6j0n2p4 \
  -H 'Content-Type: application/json' \
  -d '{"response":{"templated":true,"content_type":"application/json","body":{"encoding":"utf8","data":"{\"order\":{{ json(\"$.order.id\") }},\"request\":\"{{ capture.id }}\"}"}}}'
```

```json
{
  "name": "Refund mock",
  "condition": {
    "all": [
      { "value": "request.method", "op": "eq", "arg": "POST" },
      { "value": "request.header.x-event-type", "op": "eq", "arg": "refund.created" }
    ]
  },
  "steps": [
    { "kind": "extract", "from": "request.body", "json_path": "$.refund.id", "into": "refund_id" },
    { "kind": "increment", "counter": "refunds" },
    { "kind": "respond", "status": "201", "content_type": "application/json",
      "headers": [{ "name": "X-Refund", "value": "{{ var.refund_id }}" }],
      "body": "{\"refund\":{{ var.refund_id | json }},\"seen\":{{ counter.refunds }}}" },
    { "kind": "http", "name": "crm", "method": "POST", "url": "https://crm.example.com/hooks/refund",
      "headers": [{ "name": "Authorization", "value": "Bearer {{ secret.CRM_TOKEN }}" }],
      "body": "{{ request.body }}" },
    { "kind": "notify", "channel_id": "0193a1c2-1111-7a3e-8b44-1c2d3e4f5a6b",
      "message": "Refund {{ var.refund_id }} answered, CRM said {{ step.crm.status }}" }
  ]
}
```

The first three steps run before the response; `http` and `notify` run after it (architecture §22).
