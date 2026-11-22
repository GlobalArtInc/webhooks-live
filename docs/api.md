# webhooks.live HTTP API

Status: draft 3, 2026-11-22 (parity routes §9–§25, aligned with the shipped code). Requirements: [product-spec.md](product-spec.md). Design: [architecture.md](architecture.md).
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
| Caching | All `/api` responses: `Cache-Control: no-store`, except `GET /api/v1/openapi.json` (§23). |

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
| `POST /api/v1/endpoints/{id}/captures/{capture_id}/resend` | editor | ForwardRequest | Alias of `/forward`: same handler, body and answers |
| `GET /api/v1/endpoints/{id}/stream` | viewer | `Last-Event-ID` header or `?last_event_id=` | 200 `text/event-stream`, §6 |

Reading an endpoint, its captures or its stream extends its lifetime (E3) when `last_active_at` is older
than 60 s; captures always extend it. `PUT .../members` with an email
nobody has signed in with answers 404 `user_not_found`; the owner cannot add themself (422). Every rule change
also emits `endpoint_updated` on the stream. With forwarding switched off (`features.forwarding` false) the
forward-rule routes, `/forward` and `/resend` answer 404 `not_found`.

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

In `sso` mode the same calls need the session cookie, and mutating ones an `Origin` header; scripts use an
API token instead (§11).

## 9. Parity additions: conventions

Sections 9–24 add the parity routes (product-spec §13–§28, architecture §16–§36). §1–§8 stay valid except where
a row below extends them. The served OpenAPI document (§23) states the same contract per route; a disagreement
between the two is a bug in one of them.

| Topic | Rule |
|---|---|
| Mode `mixed` | Anonymous callers work as in `open` on anonymous endpoints; owned endpoints follow `sso` rules. Person-only routes answer 403 `sign_in_required` to anonymous callers. |
| Tokens | `Authorization: Bearer whl_...` on the routes of §11.1 only, the endpoint stream included. With a Bearer header the cookie is ignored and the origin check is skipped. Every other route answers a token 403 `session_required` before the endpoint is looked up. |
| Grants | Anonymous endpoints with a view password need `X-Webhooks-Live-Grant: <grant>` on every route whose path contains `{id}`, except `POST .../unlock` and `DELETE /folders/{folder_id}/endpoints/{id}`; the endpoint stream also accepts `?grant=`, the inbox list reads only the header. Missing or expired: 401 `grant_required`. |
| CORS | `/api/v1/*` answers preflights 204 and every response with `Access-Control-Allow-Origin: *`, never with credentials (architecture §19). Methods `GET, POST, PUT, PATCH, DELETE`; allowed request headers `Authorization`, `Content-Type`, `If-None-Match`, `Last-Event-ID`, `X-Webhooks-Live-Grant`; exposed `ETag`, `Location`, `RateLimit-Limit`, `RateLimit-Remaining`, `Retry-After`; `Access-Control-Max-Age: 600`. |
| Body limits | `POST /api/v1/send`: 1.5 MiB. Imports: the tier's import size, `application/json` or `application/x-ndjson`. Others: §1. |
| Templates in JSON | Template fields are plain strings in the §21 syntax of architecture; field errors carry `line` and `column`. |
| Switched-off features | Forwarding, notifications (switched off, or no secrets keyring) and the composer answer 404 `not_found` on their routes, as if absent. Schedules, push and everything that needs the secrets keyring answer 503 `feature_disabled`. With rules switched off (`WEBHOOKS_LIVE_RULES_ENABLED=false`) rule CRUD still works, nothing runs, and dry run and re-run answer 503 `feature_disabled`. |

| Status | `code` | When |
|---|---|---|
| 401 | `invalid_token` | Bearer token malformed, unknown, expired or revoked; `WWW-Authenticate: Bearer error="invalid_token"` |
| 401 | `grant_required` | View-protected endpoint without a valid grant |
| 401 | `wrong_password` | Unlock with a wrong password; also for an unknown or unprotected endpoint |
| 403 | `sign_in_required` | Person-only feature, anonymous caller |
| 403 | `session_required` | Route refuses tokens (§11.1) |
| 403 | `insufficient_scope` | Token scope or endpoint limit too narrow |
| 403 | `claim_refused` | Claim without the proof of architecture §17 |
| 403 | `tier_not_allowed` | Feature not in the tier that applies (`error.limit.name` names it); an endpoint without an owner has the `anonymous` tier |
| 403 | `creator_only` | E-mail channel changed by someone other than its creator |
| 403 | `owner_required` | Schedule request that uses secrets changed by someone other than the schedule's owner |
| 404 | `rule_not_found`, `secret_not_found`, `var_not_found`, `channel_not_found`, `schedule_not_found`, `template_not_found`, `folder_not_found`, `token_not_found`, `alias_not_found`, `part_not_found`, `email_address_not_found`, `subscription_not_found` | Unknown id in its scope, including another user's object. Composer history entries answer `not_found` |
| 404 | `invalid_token` | Verification or unsubscribe link token unknown, used or expired |
| 408 | `request_timeout` | `POST /send` body did not arrive within 10 s |
| 409 | `already_owned` | Claim of an owned endpoint |
| 409 | `claim_required` | `mixed`: member, schedule-create and import routes on an endpoint that has no owner yet |
| 409 | `alias_taken` | Alias in use or in quarantine |
| 409 | `name_taken` | Folder or template name already used by the user |
| 409 | `email_ambiguous` | `PUT .../members`: several accounts share the email |
| 409 | `secret_in_use` | Secret still referenced; `error.references` = `[{kind, id, name?}]` (§13) |
| 409 | `rerun_active` | Re-run while a run of the capture is `sync`, `queued` or `running` |
| 409 | `schedule_disabled` | Run now on a disabled schedule |
| 422 | `limit_reached` | Count limit hit; `error.limit` = `{name, max}`, names below |
| 422 | `already_registered` | Push subscription endpoint already registered |
| 422 | `hop_limit` | `POST /send` arrived with `X-Webhooks-Live-Hop` of 3 or more |
| 422 | `export_too_large` | Selection above the tier export size |
| 429 | `quota_exceeded` | Durable quota hit; `error.limit` = `{name, window, max, resets_at}` with `name` `outbound`, `exports`, `imports` or `token_creation`; `Retry-After` set |
| 429 | `rate_limited` | Also: the per-token limiter; test sends (5 per channel a minute, `Retry-After: 60`); export, import and dry-run slots (`Retry-After: 1`); unlock attempts |
| 503 | `feature_disabled` | Feature not configured or switched off (row "Switched-off features") |
| 503 | `mail_unavailable` | The verification mail could not be sent; the request is undone; `Retry-After: 5` |
| 503 | `password_busy` | Every view-password hashing slot is busy |

`limit_reached` names: `endpoints`, `never_expire`, `storage_bytes`, `rules`, `rule_reruns` (10 per capture),
`vars` (50), `secrets` (20), `aliases`, `tokens`, `folders`, `templates` (20), `schedules`, `notification_channels`
(10 per endpoint), `email_addresses` (5), `push_subscriptions` (10). Forward rules keep `forward_rule_limit` (§1.1).

| Field code | Raised for |
|---|---|
| `reserved` | Reserved alias |
| `secret_unknown` | A verifier, schedule or Telegram channel names a missing secret |
| `secret_not_allowed` | Secret referenced where secrets are refused (response templates, schedule bodies) |
| `template_invalid` | Template does not parse (templated response, schedule request); `line`, `column` added |
| `json_path_invalid`, `step_unknown` | Templated response: JSON path does not parse; `step.<name>` refers to no step |
| `cron_invalid`, `timezone_invalid`, `interval_too_short` | Schedule cron or time zone does not parse; fires more often than the tier allows |
| `host_not_allowed` | Push subscription endpoint outside the accepted push hosts |

Rule validation (§15) reports only the codes of §1.1 (`required` to `not_allowed`, `target_refused`,
`target_unresolvable`) with the path inside the rule (`steps.2.url`): a template, regex, JSON path or script that
does not parse in a rule is `invalid_format`, its position in the message. In notification channels an unverified
recipient is `not_allowed` on `config.address_id`, a Telegram `token_secret` naming no secret is `invalid_format`,
and a Slack or Discord URL outside its pattern is `invalid_format`.

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
| `limits.*` | integer \| null | Adds the caller's tier: `max_captures_settable`, `max_retention_seconds`, `never_expire_endpoints`, `endpoints`, `storage_bytes` (null: no owner quota), `rules_per_endpoint`, `steps_per_rule`, `sync_http_steps` (0 in v1, §15), `aliases`, `tokens`, `folders`, `schedules`, `schedule_min_interval_seconds`, `exports_per_hour`, `export_max_bytes`, `imports_per_day`, `import_max_bytes`, `outbound_per_hour` and `notifications_per_hour` (per user when signed in, per endpoint when anonymous; null: the tier grants none, the feature is unavailable to this caller), `email_notifications_per_day` |
| `features.secrets` | boolean | Secrets keyring configured |
| `features.schedules` | boolean | Schedules on and the tier allows at least one |
| `features.notifications` | string[] | Channel kinds of the caller's tier; empty when notifications are off |
| `features.push_public_key` | string \| null | VAPID public key, base64url; `null` while push is not prepared |

Planned, not served yet: `features.rules`, `features.scripts`, `features.api_cors`, `features.mail_domain`,
`features.dns_zone`, `features.subdomain_host`. Clients treat an absent feature as unknown and rely on the route
answers of §9.

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/usage` | signed in (else 401); a token needs `read` and no endpoint limit (else 403 `insufficient_scope`) | – | 200 Usage |
| `POST /api/v1/endpoints/{id}/claim` | signed in, `mixed`, session only | – | 200 Endpoint with `my_role: "owner"`; 403 `sign_in_required`, 403 `claim_refused` (caller outside the creating network, endpoint older than 24 hours, or no recorded network), 409 `already_owned` (caller is the owner or a member), 410 `endpoint_gone` (including an endpoint owned by someone else), 422 `limit_reached` (`endpoints`) |

Usage: `{tier: string, items: [{name: string, window: "hour" | "day" | "total", used: integer, max: integer, resets_at: timestamp | null}]}`.
Names: `endpoints`, `never_expire`, `schedules`, `tokens`, `storage_bytes` (window `total`), `outbound`,
`notifications`, `exports` (window `hour`).

## 11. API tokens

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/tokens` | session | – | 200 `{items: ApiToken[]}`, revoked and expired ones included until purged, at most 500 |
| `POST /api/v1/tokens` | session | ApiTokenInput | 201 CreatedApiToken |
| `DELETE /api/v1/tokens/{token_id}` | session | – | 204 (revokes; again 204 for a revoked token); 404 `token_not_found` for unknown ids and other users' tokens |

| ApiToken field | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `name` | string | 1–64 chars |
| `prefix` | string | First 12 characters of the token, the lookup key |
| `scopes` | string[] | 1–5 of `read`, `endpoints:write`, `captures:write`, `rules:write`, `send` |
| `endpoint_ids` | string[] \| null | Limit to these endpoints, 1–50; `null` = no limit |
| `created_at`, `expires_at` | timestamp | |
| `last_used_at`, `revoked_at` | timestamp \| null | |

ApiTokenInput: `{name, scopes, endpoint_ids?, expires_in_days}` with `expires_in_days` an integer 1–365 (default 90).
CreatedApiToken = ApiToken plus `token` (the only time it is returned). At most 20 active tokens per user (422
`limit_reached`, `tokens`) and 10 creations per hour (429 `quota_exceeded`, `{name: "token_creation", window:
"hour", max: 10, resets_at}`). Each token has its own limiter at the management API rate (429 `rate_limited` with
`Retry-After`). A malformed or repeated `Authorization` header answers 401, never anonymous access. A token with an
endpoint limit sees only those endpoints in `GET /endpoints`, gets 403 `insufficient_scope` on `POST /endpoints`,
on `GET /usage` and on any other endpoint.

### 11.1 Token scopes

| Scope | Routes |
|---|---|
| `read` | `GET` of `/instance`, `/openapi.json`, `/usage`, `/endpoints`, `/aliases/{alias}/availability`, `/send/history`; on an endpoint: `GET` of the endpoint, `/stream`, `/captures`, `/captures/latest`, `/captures/{capture_id}` with `/body`, `/parts/{index}` and `/rule-runs`, `/rule-runs`, `/forward-rules`, `/secrets`, `/aliases`; `POST .../unlock` |
| `endpoints:write` | `POST /endpoints`, `PATCH` and `DELETE /endpoints/{id}`, `PUT` and `DELETE .../secrets/{name}` |
| `captures:write` | `DELETE .../captures`, `DELETE .../captures/{capture_id}` |
| `rules:write` | Forward rules only: `POST .../forward-rules`, `PATCH` and `DELETE .../forward-rules/{rule_id}` |
| `send` | `POST /send`, `DELETE /send/history/{sent_id}`, `POST .../captures/{capture_id}/forward` and `/resend` |
| session only | Every other route: tokens, members, claim, alias `PUT`/`DELETE`, rules of §15 (also their `GET`), variables, notifications, push, e-mail addresses, schedules, folders, templates, clone, stats, capture `PATCH`, re-run, export, import, both inbox routes |

The effective right is `min(role on the endpoint, scopes)`.

## 12. Endpoint additions

Endpoint (§4.1) gains:

| Field | Type | Notes |
|---|---|---|
| `tags` | string[] | At most 10, each `^[a-z0-9][a-z0-9-]{0,31}$` |
| `retention` | `{mode: "sliding" \| "never", seconds: integer \| null}` | `seconds` 3600 to the tier max, `null` = instance default. `never` only for signed-in owners within the tier count: `open` answers 422 `not_allowed` on `retention.mode`, an anonymous caller in `mixed` 403 `sign_in_required`, a full count 422 `limit_reached` (`never_expire`) |
| `expires_at` | timestamp \| null | `null` for never-expire endpoints |
| `max_captures` | integer | Writable by the owner, 1 to the tier max; `null` resets to the instance default |
| `capture_ttl_seconds` | integer \| null | At least 60 |
| `sender_allow_cidrs` | string[] | At most 20 CIDRs |
| `capture_auth` | CaptureAuth \| null | Output: `{kind: "basic", username}` or `{kind: "header", name}`; secrets never returned |
| `view_protected` | boolean | Read-only; true when a view password is set |
| `view_grant` | `{grant, expires_at}` | Only in the answer to a create or `PATCH` that sets `view_password`, so the setter needs no unlock |
| `response.templated` | boolean | Default false |
| `response.status_template` | string \| null | Template for the status, up to 1024 characters; used only when `response.templated` is true; the integer `response.status` stays the fallback status |
| `verifier` | Verifier \| null | |

Planned, not in this release: `email` and `dns` (EndpointInput refuses them with 422 `not_allowed`), `aliases`,
`subdomain_url`, `rule_count`. Aliases are listed by `GET .../aliases`.

EndpointInput adds `tags`, `retention`, `max_captures`, `capture_ttl_seconds`, `sender_allow_cidrs`, `capture_auth`,
`view_password` (string 8–128 chars, write-only, `null` removes; anonymous endpoints only, else `not_allowed`),
`response.templated`, `response.status_template`, `verifier`. `POST /endpoints` also accepts `template_id`
(session only, else 403 `session_required`; 404 `template_not_found`); `PATCH` refuses it with `not_allowed`.

CaptureAuth input: `{"kind": "basic", "username": string, "password": string}` or
`{"kind": "header", "name": string, "value": string}`; `null` removes. Values 1–256 bytes, visible ASCII.

Verifier: `{"scheme": "hmac" | "github" | "stripe" | "slack" | "shopify" | "standard_webhooks", "secret_name": string,
"header": string | null, "algorithm": "sha1" | "sha256" | "sha512" | null, "encoding": "hex" | "base64" | null,
"prefix": string | null, "timestamp_header": string | null}`. Presets fill the nullable fields; `hmac` requires
`header`, `algorithm`, `encoding`. The verifier is replaced as a whole. It names a secret of the endpoint, which a
new endpoint does not have yet, so a create with a verifier answers `secret_unknown`: set it with `PATCH` after
`PUT .../secrets/{name}`. Without the secrets keyring: 503 `feature_disabled`. `CaptureSummary.verification` is
`valid`, `invalid` (also when the secret is missing or unusable) or `missing` (no signature). `github` and `shopify`
carry no timestamp, so they have no replay protection; timestamps are integer seconds of at most 12 digits;
base64 signatures use the standard alphabet only.

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints` | signed in | adds `?tag&folder`; there is no `q` | as §5 |
| `POST /api/v1/endpoints/{id}/unlock` | anyone with the id; a token needs `read` | `{password}` | 200 `{grant, expires_at}`; 401 `wrong_password`; 429 |
| `POST /api/v1/endpoints/{id}/clone` | viewer on source; `sso`/`mixed`: signed in; session only | optional `{title?, include?: ("response" \| "forward_rules" \| "rules" \| "channels" \| "schedules")[]}`; empty body = everything, `secrets` in `include` is refused | 201 Endpoint plus `secrets_missing: string[]` (secret names the copy uses and lacks) and `skipped: string[]` (parts not copied; `channels` when the caller is not an editor of the source), `Location`; 422 `limit_reached` (`endpoints`, `schedules`) |
| `GET /api/v1/endpoints/{id}/aliases` | viewer | – | 200 `{items: Alias[]}` |
| `PUT /api/v1/endpoints/{id}/aliases/{alias}` | owner, signed in, session only | – | 200 Alias; 409 `alias_taken`; 422 `validation_failed` (field code `reserved` or `invalid_format` on `alias`), 422 `limit_reached` (`aliases`) |
| `DELETE /api/v1/endpoints/{id}/aliases/{alias}` | owner, session only | – | 204; 404 `alias_not_found` |
| `GET /api/v1/aliases/{alias}/availability` | signed in; a token needs `read` | – | 200 `{alias, available: boolean, reason: "reserved" \| "invalid" \| "taken" \| null}` for any text |
| `GET /api/v1/endpoints/{id}/stats` | viewer, session only | `?bucket=hour\|day` (default `day`) | 200 Stats |
| `PATCH /api/v1/endpoints/{id}/captures/{capture_id}` | editor, session only | `{note?: string \| null, starred?: boolean}`; `null` or `""` removes the note, at most 2000 chars | 200 CaptureSummary |
| `GET /api/v1/folders` | signed in | – | 200 `{items: Folder[]}` |
| `POST /api/v1/folders` | signed in | `{name}` | 201 Folder; 409 `name_taken`; 422 `limit_reached` (`folders`) |
| `PATCH /api/v1/folders/{folder_id}` | signed in | `{name?, position?}` | 200 Folder |
| `DELETE /api/v1/folders/{folder_id}` | signed in | – | 204 |
| `PUT /api/v1/folders/{folder_id}/endpoints/{id}` | signed in, viewer on the endpoint | – | 204 |
| `DELETE /api/v1/folders/{folder_id}/endpoints/{id}` | signed in | – | 204 |
| `GET /api/v1/templates` | signed in | – | 200 `{items: TemplateSummary[]}` (without `config`) |
| `POST /api/v1/templates` | signed in, viewer on `from_endpoint_id` (else 410 `endpoint_gone`) | `{name, from_endpoint_id}` | 201 Template; 409 `name_taken`; 422 `limit_reached` (`templates`, 20) |
| `GET /api/v1/templates/{template_id}` | signed in | – | 200 Template |
| `PATCH /api/v1/templates/{template_id}` | signed in | `{name}` | 200 Template |
| `DELETE /api/v1/templates/{template_id}` | signed in | – | 204 |

Folder and template routes are session only. Alias: `{alias, url, created_at}` with `url` = `<public url>/a/<alias>`.
Folder: `{id, name, position, endpoint_ids: string[], created_at}`. TemplateSummary: `{id, name, created_at,
updated_at}`; Template adds `config`, the configuration document of architecture §24 (channel URLs and secret
values are never stored). Stats: `{bucket, from, to, series: [{at: timestamp, total, by_method: {<method>: n},
by_kind: {<kind>: n}, by_status: {"2xx": n, ..., "other": n}}]}`, oldest first; `from` and `to` are `null` for an
empty series.

## 13. Secrets

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/secrets` | editor; a token needs `read` | – | 200 `{items: [{name, created_at, updated_at}]}` |
| `PUT /api/v1/endpoints/{id}/secrets/{name}` | editor; a token needs `endpoints:write` | `{value: string}` (1–4096 bytes) | 200 `{name, created_at, updated_at}`; 422 `limit_reached` (`secrets`, 20) |
| `DELETE /api/v1/endpoints/{id}/secrets/{name}` | editor; a token needs `endpoints:write` | – | 204; 404 `secret_not_found` (checked first); 409 `secret_in_use` |

Values are never returned. Name: `^[A-Z][A-Z0-9_]{0,63}$`; a name outside the pattern answers 404 like an unknown
one. Without the secrets keyring every route answers 503 `feature_disabled`. `secret_in_use` lists every holder in
`error.references` as `{kind, id, name?}`: `verifier` (`id` = endpoint id, no `name`), `rule` (a step or condition,
enabled or not), `channel` (a Telegram `token_secret`), `schedule` (URL or header); `rule`, `channel` and
`schedule` carry the object's `id` and `name`. Forward rules hold no secrets. The check and the delete run in one
transaction under the endpoint lock. A stored rule that no longer compiles makes the check fail with 500 and blocks
the delete until the rule is fixed or deleted.

## 14. Persistent variables

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/vars` | viewer | – | 200 `{items: [{name, value, updated_at}]}` |
| `PUT /api/v1/endpoints/{id}/vars/{name}` | editor | `{value: any JSON}` (at most 4 KiB serialized) | 200 Var; 422 `limit_reached` (`vars`, 50) |
| `DELETE /api/v1/endpoints/{id}/vars/{name}` | editor | – | 204; 404 `var_not_found` |

Session only. Name `^[a-z_][a-z0-9_]{0,63}$`, at most 50. Counters are variables holding an integer.

## 15. Rules

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/rules` | viewer | – | 200 `{items: Rule[], version: integer}` |
| `POST /api/v1/endpoints/{id}/rules` | editor | RuleInput | 201 Rule (appended last); 422 `limit_reached` (`rules`) |
| `GET /api/v1/endpoints/{id}/rules/{rule_id}` | viewer | – | 200 Rule |
| `PATCH /api/v1/endpoints/{id}/rules/{rule_id}` | editor | RuleInput (partial) | 200 Rule |
| `DELETE /api/v1/endpoints/{id}/rules/{rule_id}` | editor | – | 204 |
| `PUT /api/v1/endpoints/{id}/rules/order` | editor | `{ids: uuid[]}` (every rule exactly once) | 200 `{items: Rule[], version}` |
| `POST /api/v1/endpoints/{id}/rules/dry-run` | editor | `{capture_id, rules?: RuleInput[]}` (at most 5 drafts) | 200 RuleRun (`trigger: "dry_run"`, not stored); 404 `capture_not_found`; 429 `rate_limited` while another dry run of the endpoint runs |
| `GET /api/v1/endpoints/{id}/captures/{capture_id}/rule-runs` | viewer | – | 200 `{items: RuleRun[]}` newest first |
| `POST /api/v1/endpoints/{id}/captures/{capture_id}/rule-runs` | editor | – | 202 RuleRun (`trigger: "rerun"`, `state: "queued"`); 409 `rerun_active`; 422 `validation_failed` on `rules` (no enabled rule), 422 `limit_reached` (`rule_reruns`, 10 per capture) |
| `GET /api/v1/endpoints/{id}/rule-runs` | viewer; a token needs `read` | `?limit` (1–200, default 50), `cursor`, `state` | 200 `{items: RuleRun[], next_cursor}` for every capture of the endpoint, newest first; `next_cursor` is the id of the last run; 400 `invalid_cursor` |

Rule CRUD, dry run and re-run are session only (§11.1); the run lists accept `read` tokens. With rules switched
off, dry run and re-run answer 503 `feature_disabled`. Every change emits `endpoint_updated` and bumps `version`,
one counter for all rules of the endpoint. A rule save locks the endpoint row, so it waits for captures being stored
on that endpoint.

**Edit conflicts (planned).** Today a client detects a concurrent change only by re-reading the list before it
saves; a change between that read and the save is overwritten. Decided contract, on the existing `version`:

| Part | Contract |
|---|---|
| `ETag` | `GET .../rules`, `GET .../rules/{rule_id}` and every successful rule mutation answer `ETag: "rules-<version>"`, the version after the change |
| `If-Match` | Optional on `POST .../rules`, `PATCH` and `DELETE .../rules/{rule_id}`, `PUT .../rules/order`. A tag different from the current one changes nothing and answers 412 `version_conflict` with `error.current_version`; no header or `If-Match: *` writes unconditionally, as today |
| CORS | `If-Match` joins the allowed request headers (§9) |

Why: `version` is already bumped in the transaction of every change, under the endpoint row lock the save takes, so
the check is one comparison; a header leaves RuleInput unchanged. Rejected: a per-rule revision (reorder and the
plan cut depend on all rules at once) and keeping re-read semantics (two editors overwrite each other silently).

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
read its result) and `continue_on_error` (default false). Unknown fields anywhere in a rule answer `not_allowed`.

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
| `http` | `method`, `url` (template), `headers` (`{name, value}[]`, templated), `body` (template \| null), `timeout_ms` (at most 15000, default 10000), `follow_redirects` (accepted and ignored: rule HTTP steps never follow redirects, a 3xx answer is data in `step.<name>.status` and its headers). After the response only |
| `forward` | `forward_rule_id`, or `target_url` with optional `method`, `content_type`, `header_allowlist`; `body_from`: `"capture"` (default) or `"payload"` |
| `notify` | `channel_id`, `message` (template \| null: channel default) |
| `store_s3` | `bucket_url`, `prefix`, `region`, `access_key_secret`, `secret_key_secret`, `content`: `"capture_json"` or `"body"` |

**Before and after the response.** The plan is cut after the last `respond` step (architecture §22). `forward`,
`notify` and `store_s3` before the cut are deferred and run after the response with the variables of that moment.
`http` steps may stand only after the cut: a write that puts an `http` step before it (create, update, enable,
reorder) is refused with 422 `validation_failed`, field code `not_allowed` on that step's `kind` (`steps.N.kind` for
the rule being changed, `rules.R.steps.N.kind` for another rule that the write pushes before the cut). A set that
already holds such a step from before this rule existed can still be edited: a write is refused only when the
violation is in the rule being changed or did not exist before, and a delete is never refused. Why: the
before-response part has no HTTP budget in v1, and an accepted rule that fails every capture is a trap; refusing at
save shows the error where it can be fixed. Rejected: silently moving the step after the response (its result
would be missing for the response it was placed to shape). `limits.sync_http_steps` is 0 in v1; HTTP before the
response for higher tiers is planned. A stored set that still has such a step fails each synchronous run with
`X-Webhooks-Live-Rule-Error: http`.

**Not in v1.** Per-rule capture-kind triggers, an "on match" choice and per-step guards (`if`) are not part of the
rule model; such fields answer `not_allowed`. Equivalents: a leaf `{"value": "request.kind", "op": "in", "arg":
["http", "email"]}` for a trigger, a final `stop` step for "stop here", a separate rule with its own condition for
a guarded step.

**Dry run.** Runs the saved rules, or up to 5 unsaved drafts in place of all saved rules, against a stored capture
inside the request: at most 10 s, one dry run per endpoint at a time, nothing stored, nothing sent. State steps
log `no_op` with "would set ..."; side effects before the cut log `deferred`; after the cut, response steps log
`no_op` and `http`, `forward`, `notify`, `store_s3` log `no_op` with the request or message that would go out.
`execute_http` is accepted for compatibility, must be a boolean and has no effect: a dry run never sends HTTP. Why:
a test click must not spend the outbound quota or reach third parties; the rendered request answers what the
tester asks. Rejected: implementing `execute_http` (a second outbound path to guard and charge). The answer is a
RuleRun with `trigger: "dry_run"` and `id` = the capture id. Every step of every matched rule appears in
`entries`, also in rules without a `respond` step and after the cut. Secrets are never resolved: `{{ secret.NAME }}`
renders as `[secret:NAME]`. An error after the cut is recorded in that entry and does not change the run's state or
`response_status` (as in a live run, where the response is already sent); a rule condition that fails after the cut
reports `matched: false` with the error text. When the 10 s limit is reached the entries collected so far are
returned, the last one naming the limit.

RuleRun:

| Field | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `capture_id` | uuid | |
| `trigger` | `"capture"` \| `"rerun"` \| `"dry_run"` | |
| `state` | `"sync"` \| `"queued"` \| `"running"` \| `"succeeded"` \| `"failed"` | |
| `rules_version` | integer | |
| `response_status` | integer \| null | Status the rules produced |
| `error` | string \| null | `template`, `budget`, `script`, `http`, `quota_exceeded`, `overloaded`, `hop_limit`, `relayed`, `internal`, then `: ` and detail |
| `entries` | RunEntry[] | In execution order |
| `created_at`, `finished_at` | timestamp, timestamp \| null | |

RunEntry: `{rule_id, rule_name, matched: boolean}` for a rule, or `{rule_id, step: integer, name: string | null,
kind, outcome: "ok" | "failed" | "skipped" | "deferred" | "no_op", duration_ms, detail: string | null, output: any | null}`
for a step. `output` holds at most 4 KiB: the HTTP result `{status, headers, body: Body, duration_ms}`, extracted
values, or script output; secrets appear as `[secret:NAME]`.

| Step case | Entry |
|---|---|
| `notify` on a re-run when the capture already has a delivery for that channel | `skipped`, detail "a delivery for this channel and capture already exists, nothing was sent": one delivery per channel and capture |
| `notify` on a relayed capture (`X-Webhooks-Live-Hop` 1 or more) | `failed`, run error `relayed`: relayed captures never notify, as for capture-triggered channels |
| `notify` enqueue conflict without a stored delivery | `failed`, `internal: delivery_exists: ...` (never expected; logged) |
| Outbound step over quota | `failed`, `quota_exceeded`: rules charge the endpoint owner per user and hour, or for an ownerless endpoint its network per day and the endpoint per hour |
| Capture at hop 3 or more | No run is created |

## 16. Notifications

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/notifications` | viewer | – | 200 `{items: Channel[]}` |
| `POST /api/v1/endpoints/{id}/notifications` | editor | ChannelInput | 201 Channel; 422 `limit_reached` (`notification_channels`, 10) |
| `PATCH /api/v1/endpoints/{id}/notifications/{channel_id}` | editor | ChannelInput (partial; `kind` refused with `not_allowed`) | 200 Channel |
| `DELETE /api/v1/endpoints/{id}/notifications/{channel_id}` | editor | – | 204 |
| `POST /api/v1/endpoints/{id}/notifications/{channel_id}/test` | editor | – | 202 Delivery (`kind: "test"`); 5 per channel a minute, then 429 `rate_limited`, `Retry-After: 60` |
| `POST /api/v1/notifications/disable` | none | `{token}` or `?token=` (one-click `List-Unsubscribe-Post`) | 204 (channel disabled, kept); 404 `invalid_token` |
| `GET /api/v1/email-addresses` | signed in | – | 200 `{items: EmailAddress[]}` |
| `POST /api/v1/email-addresses` | signed in | `{address}` | 201 for a new address (verification sent), 200 for one already on the account (a new mail may be sent); 422 `limit_reached` (`email_addresses`, 5); 503 `mail_unavailable` |
| `DELETE /api/v1/email-addresses/{address_id}` | signed in | – | 204 |
| `POST /api/v1/email-addresses/verify` | none | `{token}` or `?token=` | 200 EmailAddress; 404 `invalid_token` |
| `GET /api/v1/push/public-key` | none | – | 200 `{public_key}` (base64url) |
| `GET /api/v1/push/subscriptions` | signed in | – | 200 `{items: PushSubscription[]}` |
| `POST /api/v1/push/subscriptions` | signed in | browser `PushSubscription` JSON `{endpoint, keys: {p256dh, auth}}`; `expirationTime` accepted and ignored | 201 `{id, created_at}`; 422 `already_registered`, 422 `limit_reached` (`push_subscriptions`, 10), field `host_not_allowed` |
| `DELETE /api/v1/push/subscriptions/{subscription_id}` | signed in | – | 204 |

All routes except the two link routes and the public key are session only. With notifications switched off (or
no secrets keyring) the channel, e-mail-address and link routes answer 404 `not_found`; push routes answer 503
`feature_disabled` while push is not prepared (architecture §25). `email` and `push` channels need a signed-in
caller (403 `sign_in_required`); `push` needs an instance with accounts (403 `not_available_in_mode`). A kind
outside the applicable tier answers 403 `tier_not_allowed` with `error.limit.name` = the kind, checked before the
body's fields.

| Channel field | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `kind` | `"webhook"` \| `"slack"` \| `"discord"` \| `"telegram"` \| `"email"` \| `"push"` | |
| `name` | string | 1–64 chars |
| `enabled` | boolean | Default true |
| `trigger` | `"capture"` \| `"rule"` | Default `capture`; `rule` channels are used only by notify steps |
| `condition` | null | Always `null`; a non-null value answers `not_allowed` (channel conditions are planned) |
| `config` | object | Per kind, below |
| `template` | string \| null | Message template; `null` = default |
| `per_hour` | integer | 1–3600, default 30; tier caps apply at send time as owner quotas |
| `created_at` | timestamp | |

| `kind` | `config` |
|---|---|
| `webhook` | `{url}`, checked like a forward target and refused when it points at this instance; returned masked as `scheme://host/...` |
| `slack` | `{url}` matching the Slack incoming-webhook pattern (else `invalid_format`); sealed, returned masked (`https://hooks.slack.com/services/...`) |
| `discord` | `{url}` matching the Discord webhook pattern; sealed and returned like Slack |
| `telegram` | `{token_secret, chat_id}`; `token_secret` is the name of an endpoint secret and is returned as such; `chat_id` numeric or `@username` |
| `email` | `{address_id}`, a verified address of the caller. Only the creator may change the channel (403 `creator_only`). On an endpoint without an owner the answer is 403 `tier_not_allowed` (`email`): that endpoint has the `anonymous` tier, which has no e-mail, and the tier check runs before field validation |
| `push` | `{}`: the creator's subscriptions |

Delivery: `{id, channel_id, kind: "single" | "summary" | "test", state: "pending" | "running" | "succeeded" | "failed" | "suppressed", attempts, status: integer | null, error: string | null, created_at, finished_at}`.
EmailAddress: `{id, address, verified: boolean, created_at}`. PushSubscription: `{id, host, user_agent: string |
null, created_at, last_success_at: timestamp | null}`; the endpoint URL and keys are never returned. The UI has no
flow that deletes a subscription yet; the server deletes one when the push service answers 404 or 410.

## 17. Composer

| Method, path | Role | Request | Success |
|---|---|---|---|
| `POST /api/v1/send` | `open`/`mixed`: anyone; `sso`: signed in; a token needs `send` | SendRequest | 200 SendResult, also when the delivery failed |
| `GET /api/v1/send/history` | signed in, or `?endpoint_id=` with viewer; a token needs `read` | `?endpoint_id&cursor&limit` (1–50, default 50) | 200 `{items: SentRequest[], next_cursor}`, newest first |
| `DELETE /api/v1/send/history/{sent_id}` | the signed-in owner of the entry, or an editor of its endpoint; a token needs `send` | – | 204; 404 `not_found` for unknown or foreign entries |

With the composer off every route answers 404 `not_found`. SendRequest: `{method, url, headers: {name, value}[],
body: Body | null, timeout_ms?: integer (≤ 15000, default 15000), follow_redirects?: boolean (default false; at
most 5), endpoint_id?: string, source_capture_id?: uuid}`. `method` is `GET`, `HEAD`, `POST`, `PUT`, `PATCH`,
`DELETE` or `OPTIONS`; `url` is `http`/`https` without user information and gets the forward-target field codes
(§5.3); a capture URL of this instance is delivered inside the instance (`capture_id` set), other URLs of this
instance are refused. Hop-by-hop headers, `Host`, `Forwarded`, `X-Forwarded-*`, `X-Real-IP`, `X-Client-IP`,
`True-Client-IP`, `CF-Connecting-IP`, `Proxy-*` and `X-Webhooks-Live-*` answer `not_allowed`. 408
`request_timeout` when the body does not arrive within 10 s; 422 `hop_limit` at hop 3 or more.

SendResult: `{id, status: integer | null, headers: {name, value}[], body: Body | null (first 1 MiB), duration_ms,
final_url: string | null, error: string | null, capture_id: uuid | null}`. `error` is `<code>: <detail>` with codes
`timeout`, `refused_target`, `refused_redirect`, `dns_failure`, `connect_timeout`, `hop_timeout`, `total_timeout`,
`too_many_redirects`, `transport`, `configuration`, `local_delivery`. Each send charges `outbound`: a signed-in
caller per hour, an anonymous caller per network per day and, with `endpoint_id`, per endpoint per hour.

History: sends of a signed-in caller are kept under the user (also with `endpoint_id`), anonymous sends with
`endpoint_id` under the endpoint, newest 50 each. SentRequest = SendResult plus `created_at` and `request`, the
SendRequest stored redacted (sensitive header and query values and user information replaced; an endpoint's
entries keep only the URL origin) with `body_size`; a response body is kept only for JSON and form-encoded
responses.

## 18. Schedules

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/schedules` | viewer | – | 200 `{items: Schedule[]}` |
| `POST /api/v1/endpoints/{id}/schedules` | owner, signed in | ScheduleInput | 201 Schedule; 409 `claim_required`; 422 `limit_reached` (`schedules`) |
| `PATCH /api/v1/endpoints/{id}/schedules/{schedule_id}` | editor | ScheduleInput (partial); `enabled: true` clears a pause and resets `consecutive_failures` | 200 Schedule; 403 `owner_required` |
| `DELETE /api/v1/endpoints/{id}/schedules/{schedule_id}` | editor | – | 204 |
| `POST /api/v1/endpoints/{id}/schedules/{schedule_id}/run` | editor | – | 202 `{schedule_id, queued_at}`; 409 `schedule_disabled` |
| `GET /api/v1/endpoints/{id}/schedules/{schedule_id}/runs` | viewer | – | 200 `{items: ScheduleRun[]}`, the 50 newest |

Session only; instances without accounts answer 403 `not_available_in_mode`; schedules switched off: 503
`feature_disabled`.

| Schedule field | Type | Notes |
|---|---|---|
| `id` | uuid | |
| `name` | string | 1–100 chars |
| `cron` | string | 5 fields; `cron_invalid`, `interval_too_short` |
| `timezone` | string | IANA name, default `UTC` |
| `request` | `{method, url, headers: {name, value}[], body: string \| null}` | Templates; secrets (`secret.<NAME>`) allowed in `url` and header values (`secret_unknown`), at most 20 headers, body at most 64 KiB; SSRF refusal is `target_refused` |
| `enabled` | boolean | Default true |
| `paused_reason` | string \| null | Read-only |
| `next_run_at`, `last_run_at` | timestamp \| null | Read-only |
| `consecutive_failures` | integer | Read-only |
| `created_at` | timestamp | Read-only |

ScheduleRun: `{id, started_at, outcome: "succeeded" | "failed" | "skipped", status: integer | null, duration_ms,
error: string | null, capture_id: uuid | null}`. Runs are at least once (60 s lease), never backfilled, and spaced at
least the tier interval from the scheduled time; a status of 400 or more is a failure and 10 failures in a row pause
the schedule. The instance runs at most 120 schedule runs an hour per owner. Only the schedule's owner may change a
request when the old or new one uses secrets. Redirects are not followed. Each run stores a `schedule` capture (hop
1) that counts toward the endpoint's limits and keeps it alive.

## 19. Export and import

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/export` | viewer, session only | `?format=json\|ndjson\|csv\|zip` (required) plus `method`, `status`, `kind`, `starred`, `q`, `after_seq`, `since`, `until`; `order`, `limit`, `cursor` are ignored | 200 stream, oldest first |
| `POST /api/v1/endpoints/{id}/export` | viewer, session only | `{format, capture_ids: uuid[]}` (1–500; duplicates dropped; foreign ids left out) | 200 stream |
| `POST /api/v1/endpoints/{id}/import` | editor on an owned endpoint, session only | JSON or NDJSON export; `?keep_times=true\|false` (default true) | 200 `{imported, skipped, reason: "capacity" \| "storage" \| null}`; 409 `claim_required` (`mixed`, no owner yet) |
| `POST /api/v1/endpoints/import` | signed in, session only | as above | 201 `{endpoint_id, imported, skipped, reason}`, `Location: /api/v1/endpoints/{id}`; the endpoint takes `title`, `response`, `cors`, `overflow` from the header's `endpoint` and follows `POST /endpoints` rules; a failed import deletes it |

| Format | `Content-Type` | File name |
|---|---|---|
| `json` | `application/json` | `webhooks-live-<id>-<yyyymmddHHMMSS>.json` |
| `ndjson` | `application/x-ndjson` | `.ndjson` |
| `csv` | `text/csv; charset=utf-8` | `.csv` |
| `zip` | `application/zip` | `.zip` |

Every export answers with `Content-Disposition: attachment`, `X-Content-Type-Options: nosniff`, `Cache-Control:
no-store`, chunked transfer. The JSON document and NDJSON header carry `format: "webhooks-live-export"`,
`version: 1`, `exported_at`, `endpoint` (EndpointInput fields plus `id`, without secrets). Captures carry the whole
body as `{encoding: "utf8" | "base64", data, truncated: false}`. CSV `body_preview` holds the first 4 KiB as text,
or `base64:` plus base64 when the bytes are not UTF-8 (a text body starting with `base64:` reads the same). ZIP:
`captures.ndjson` with every capture (no header line, `body` null) and `bodies/<seq>-<capture id>.bin` for every
stored body. A capture whose stored body is missing is left out of json, ndjson and csv.

Export limits: the selection's stored bytes are measured before the first byte against the tier's export size (422
`export_too_large`); quota `exports` per hour (429 `quota_exceeded`); one export per caller and 2 per replica (429
`rate_limited`, `Retry-After: 1`). Once streaming, the body ends early when the client reads nothing for 30 s,
output passes twice the tier size, or the run passes 10 minutes; a body that stops early is a failed export.

Import: `Content-Type` `application/json` or `application/x-ndjson` (else 415 before reading); the header needs
`format`, `version: 1`, `exported_at`, `endpoint`. `Content-Length` above the tier's import size answers 413 before
reading, a longer stream stops with 413; 30 s without data or more than 10 minutes answers 400 `invalid_json`. With
`keep_times=true` each `received_at` must lie between 2015-01-01 and five minutes from now. The first invalid
capture stops the import with 422 `validation_failed`, `field` = `captures.<index>.<path>` or `line.<n>`; captures
stored before it stay. A full endpoint or owner storage stops counting into `skipped` with `reason`. One import per
caller and 2 per replica (429 `rate_limited`, `Retry-After: 1`), 60 s pause after a finished import, quota `imports`
per day (429 `quota_exceeded`). Imported captures run no rules, forwards or notifications.

## 20. Combined inbox

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/inbox/captures` | viewer on each endpoint, session only | selection below; filters `method`, `status`, `kind`, `q`, `since`, `until`; `order` only `desc`; `limit` 1–50 (default 25); `cursor` | 200 `{items: CaptureSummary[], next_cursor}` newest first |
| `GET /api/v1/inbox/stream` | viewer on each endpoint, session only | same selection; `Last-Event-ID` or `?last_event_id` | 200 `text/event-stream` |

Selection: `endpoints=<ids, comma>`, at most 50 signed in and 20 anonymous (more: 422 `validation_failed`, field code `too_many`); without it a
signed-in caller gets all own and shared endpoints, at most 50 (anonymous in `mixed`: 403 `sign_in_required`;
`open` requires `endpoints`). `folder` answers 422 `not_allowed` until folders are selectable; `after_seq` answers
422 `not_allowed`; a malformed id answers 404. An explicit selection with an endpoint that is unknown or not
viewable answers 410 `endpoint_gone`; the default selection shows only what the caller may view. Reading the list
does not extend endpoint lifetimes; connecting the stream does.

The inbox stream sends §6 events for every endpoint in the set except `endpoint_updated`. Its first event is
`ready` with `{endpoints: [{endpoint_id, last_seq, capture_count}]}`; `capture` payloads and `ready` carry endpoint
ids. Event ids are opaque; resume works as §6 with at most 100 replayed events. Access is re-checked when an
`access_changed` signal arrives and every 15 s; losing access to any endpoint closes the whole stream (the client
reconnects with its new selection). To find which endpoint was lost, check each id with `GET /endpoints/{id}`.

## 21. Capture additions

CaptureSummary (§4.3) gains `kind` (`"http"` \| `"email"` \| `"dns"` \| `"schedule"`), `starred` (boolean),
`has_note` (boolean), `verification` (`"valid"` \| `"invalid"` \| `"missing"` \| null) and `imported` (boolean).
Planned, not returned yet: `rule_state` and `subject`; the capture's runs are in `rule_runs` of the detail and in
`GET .../rule-runs`. The list query (§5.1) gains `kind` (comma list) and `starred` (boolean).

Capture (§4.4) gains `note`, `meta` (per kind, below), `rule_runs` (RuleRun[], newest first, at most 10) and
`notifications` (Delivery[]).

| `kind` | `meta` |
|---|---|
| `http` | `{hop: integer}`; `template` when a templated response dropped a header or fell back (below) |
| `email` | `{envelope_from, envelope_to: string[], helo, tls: boolean, tag: string \| null, subject, from, to: string[], cc: string[], date: timestamp \| null, message_id: string \| null, parts: Part[]}`; `parse_error` when the message could not be parsed, `parts_truncated` when it had more than 50 parts |
| `dns` | `{name, type, class, transport: "udp" \| "tcp", dns_id, flags: {rd, cd, do}, edns_udp_size: integer \| null, client_subnet: string \| null}` |
| `schedule` | `{schedule_id, schedule_name, request: {method, url}, duration_ms, final_url, error, truncated}` |

Imported captures also carry `meta.original_id`.

Part: `{index, content_type, filename: string | null, size, disposition: "inline" | "attachment" | null, is_text: boolean}`.

| Method, path | Role | Request | Success |
|---|---|---|---|
| `GET /api/v1/endpoints/{id}/captures/{capture_id}/parts/{index}` | viewer | `?view=text&download=1` | 200 decoded part with the §5.2 headers; `filename` from the part, sanitised; 404 `part_not_found` for a missing index or a message that cannot be split; 503 while the decoder is busy or times out |

Body and part routes send the whole content; `Range` is not supported.

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
| `X-Webhooks-Live-Template` | A templated response fell back to the static one | `error`; `X-Webhooks-Live-Template-Error` carries the reason: `invalid`, `render`, `deadline`, `budget`, `status`, `overloaded`, `variables`, `internal` |

A templated response that falls back sends the configured status (or the path override) and the stored content
type with an empty body and none of the configured headers. Rendering has 100 ms in total and 50 ms per template;
the variables it reads are capped at 4096 compact bytes. When a templated response drops a header or falls back,
`meta.template` records `{outcome: "header_dropped" | "fallback", code, diagnostics: [{field, code, line, column}],
truncated}` with at most 20 diagnostics and no request data.

`/a/{alias}[/{rest}]` behaves exactly as `/{id}[/{rest}]`. Subdomain capture (`https://{id}.<host>/{rest}`) and
mail to aliases are planned (architecture §24, §28).

## 22. Stream additions

| Event | `id` | `data` | Client action |
|---|---|---|---|
| `rule_result` | – | `{capture_id, run: RuleRun}` | Update the rule panel |
| `captures_imported` | – | `{imported: integer}` | Re-list from the top |

Planned, not emitted yet: `capture_updated` (note, star, verification changes) and `notification_result`
(delivery state). Until then a client refreshes the capture after its own `PATCH` and reads deliveries from the
capture detail.

## 23. OpenAPI

`GET /api/v1/openapi.json`: OpenAPI 3.1 document of every `/api/v1` route (94 operations in this release), `ETag` =
`"<build version>-<8 hex of a SHA-256 of the document>"`, so it changes whenever the document does; public in all
modes. `If-None-Match` with the tag (weak or strong, or `*`) answers 304. `Cache-Control: no-cache` (the one `/api`
response that may be stored, always revalidated). A Bearer token is not needed; outside `open` a token that is sent
needs the `read` scope (403 `insufficient_scope`), and a bad one answers 401 `invalid_token`. Paths are relative
to the server `/api/v1`. Security schemes: `session_cookie` and `api_token`; operations that refuse tokens list
only `session_cookie`. Every operation with `{id}` in its path documents the optional `X-Webhooks-Live-Grant`
header and 401 `grant_required` (§25). The reference page is a UI route (ui-design.md §22.3).

Weakly typed in the document (free-form `object`, `string` or a media type): import bodies, capture body and part
downloads, export files, both streams, `openapi.json` itself, `Channel.config`, `Rule.condition`, `Rule.steps`,
`RuleRun.entries`, `Template.config`, `Var.value`, Capture and Delivery `meta`. This document describes their
shapes.

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

## 25. Annotating a route

- Each API area owns an `#[derive(OpenApi)]` struct beside its handlers (`quota.rs` has `UsageDoc`, `tokens/doc.rs` has `TokensDoc`); handlers carry `#[utoipa::path]`, DTOs carry `ToSchema`.
- Register the area with one line in `crates/whl-api/src/openapi/registry.rs` (`area_docs`, alphabetical). Nothing else is shared.
- Paths are relative to `/api/v1` and use the router's `{param}` names. Every `{param}` needs a `params(...)` entry or an `IntoParams` type (`EndpointIdParam` for `{id}`).
- `operation_id` = `<area>_<verb>` in snake case (`tokens_create`); `tag` = the area's title-case name, described in the area's `tags(...)`.
- Schema names follow this document (`#[schema(as = ApiToken)]`); doc-only request types are named after their object (`ApiTokenInput`). No field names or shapes change for the sake of the schema.
- Security: `security(("session_cookie" = []), ("api_token" = []))` when tokens may call the route, `("session_cookie" = [])` alone for session-only routes, `security(())` for public ones.
- Errors: one `responses` entry per status. Reuse `crate::openapi::responses` (`Unauthenticated`, `StoreUnavailable`, `ValidationFailed`, `BadRequest`, `PayloadTooLarge`, `UnsupportedMediaType`); when a status carries several codes, use `body = ErrorBody` and name each code in `description` as in the tables of §1 and §9. 429 and 500 are added to every operation automatically.
- Allowlist: `crates/whl-api/src/openapi/undocumented.rs` holds one constant, `UNDOCUMENTED`, and it is empty: every route is documented. A route may be parked there only for a stated reason (say it in the commit message), as `route("GET", "/path/{id}")`. The coverage test fails for a routed route that is neither documented nor listed, for a listed route that is documented or not routed, and for a documented operation without a route.
- The view-password grant is documented once, in `openapi/grant.rs`: every operation whose path contains `{id}` gets the optional `X-Webhooks-Live-Grant` header and a 401 that names `grant_required`, except `POST /endpoints/{id}/unlock` and `DELETE /folders/{folder_id}/endpoints/{id}` (the same routes `policy::grant_rule` exempts). Do not repeat the header in a handler; the two stream routes declare it themselves because they also take `?grant=`.
- Routes are registered through `crate::routing` (`Routes`, `get`, `post`, ...), which records each method and path for the coverage test; do not use `axum::routing` in an area.
- The two stream routes are served by whl-live and are not recorded by `Routes`; they are documented in `openapi/streams.rs` (`StreamsDoc`) and added to the coverage test by `streams::routes()`, built from `whl_live::STREAM_PATH` and `whl_live::INBOX_STREAM_PATH`.
