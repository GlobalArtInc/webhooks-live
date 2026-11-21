# webhooks.live HTTP API

Status: draft 1, 2026-11-21. Requirements: [product-spec.md](product-spec.md). Design: [architecture.md](architecture.md).
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
| 403 | `not_available_in_mode` | Route needs `sso` (listing, members) |
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
| `auth_mode` | `"open"` \| `"sso"` | Access mode |
| `user` | User \| null | Signed-in user; `null` when anonymous or in `open`. Never answers 401 |
| `public_url` | string | Base of capture URLs |
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
