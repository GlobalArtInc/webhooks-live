# webhooks.live product specification

Status: draft 2, 2026-11-21 (parity scope added: §8 `mixed`, §11, §13–§28).

webhooks.live is a self-hosted HTTP request catcher. A person creates an endpoint, points a webhook sender at it,
and watches every request arrive in real time with full headers, query, and body. webhooks.live replaces the
open-source PHP/Angular.js request catcher currently deployed at `webhooks.globalart.hu`.

This document describes **what the product does**, independent of any implementation. It is the contract
the Rust backend and the React frontend are built against. Architecture lives in
[architecture.md](architecture.md), the interface in [ui-design.md](ui-design.md).

## 1. Provenance

The behaviour inventory below was taken by using and reading the old application, which is MIT-licensed
(Copyright (c) 2017 Simon Fredsted). webhooks.live is a from-scratch rewrite: no source file, template, stylesheet,
image, or text string is carried over, and the product has its own name, API paths, data model, and look.
Functionality (receive any request, show it live) is not protected; the old name and branding are not used.
See [licensing.md](licensing.md).

## 2. Concepts

- **Endpoint** — a capture URL with a random identifier. It has a response configuration (what the sender
  gets back) and a retention window. Old term: "token".
- **Capture** — one stored HTTP request received by an endpoint. Old term: "request".
- **Viewer** — a person looking at an endpoint in the browser.
- **Sender** — whatever calls the capture URL (a webhook provider, curl, a script).

## 3. Capture behaviour

| # | Behaviour |
|---|---|
| C1 | Any HTTP method on `/{endpoint_id}` and on any sub-path `/{endpoint_id}/...` is captured. |
| C2 | The sender receives the endpoint's configured response: status, content type, body. Defaults are 200, `text/plain`, empty body. |
| C3 | The first path segment after the identifier, if it is a three-digit number from 100 to 599, overrides the response status for that call (`/{id}/404` answers 404). Any other sub-path is just captured. |
| C4 | The response carries `X-Request-Id` (the capture id) and `X-Endpoint-Id` headers. |
| C5 | Optional per-endpoint response delay of 0–10 seconds before answering. The capture is stored and broadcast before the delay starts. |
| C6 | Optional per-endpoint CORS mode: permissive CORS headers on the response, and automatic answers to `OPTIONS` preflight. Preflight requests are captured like any other request. |
| C7 | Stored per capture: id, endpoint id, method, full URL as seen by the server, path, hostname, sender IP, headers (multi-valued, original order), query parameters, raw body bytes, parsed form fields when the content type is a form, content type, body size, timestamp. |
| C8 | The sender IP is the real client IP, resolved from a configurable list of trusted proxy ranges (forwarded headers only trusted from those ranges). |
| C9 | An unknown or expired endpoint answers 410 Gone with a plain-text hint. A malformed identifier answers 404. |
| C10 | An endpoint holds at most N captures (default 500). Beyond that the sender gets 410 and nothing is stored, until captures are deleted or the endpoint expires. Optional ring mode (drop oldest instead of refusing) is a setting. |
| C11 | Body size limit per capture (default 10 MiB). Larger bodies are rejected with 413 and not stored. |
| C12 | Captured content is **never** served back with an executable content type from the capture domain; the raw view always uses a safe type and a sandbox policy. |

## 4. Endpoint management

| # | Behaviour |
|---|---|
| E1 | Create an endpoint with defaults, or with custom status, content type, body, response headers (at most 20, subject to the denylist in [architecture.md](architecture.md)), delay, CORS. |
| E2 | Read, update, and delete an endpoint by identifier. |
| E3 | Endpoint lifetime is sliding: any capture or view extends it. Default 7 days of inactivity, configurable. |
| E4 | Deleting an endpoint deletes all its captures. |
| E5 | The identifier is an unguessable random value (the old application used UUID v4; webhooks.live is free to use a shorter, URL-safe, equally unguessable form). Knowing the identifier is the access control for the public deployment mode. |
| E6 | A viewer can have a list of "my endpoints" in the browser (kept locally), and switch between them. |

## 5. Viewing and API

| # | Behaviour |
|---|---|
| V1 | List captures of an endpoint, paginated, newest first by default, oldest first optional. |
| V2 | Get one capture; get the latest capture; get the raw body of a capture with a safe content type. |
| V3 | Delete one capture; delete all captures of an endpoint. |
| V4 | Live updates: when a capture arrives, every viewer watching that endpoint sees it without reloading. Delivery is best-effort; a reconnecting viewer catches up by listing. |
| V5 | The live stream always carries a capture summary; the full capture is fetched on demand. |
| V6 | Deep links: a URL identifies an endpoint and optionally a selected capture. |

## 6. Viewer interface features

The new interface keeps the working set of the old one and adds what it lacked.

Kept:

- Request list with method badge, relative time, and delete; selected request details.
- Details: method, URL, sender IP, time, headers, query, form fields, raw body, size.
- Pretty-printing of JSON and XML bodies, with a raw toggle.
- First / previous / next / last navigation; optional auto-jump to the newest capture.
- Unread counter in the browser title; toast on a new capture.
- Copy endpoint URL; copy a capture as `curl` and as HAR.
- Edit endpoint response settings; CORS toggle; delete all captures.
- Forward: resend captures to another URL (see 7).
- Preferences remembered locally.

Added:

- Search and filter the list (method, status class of the override, text in headers/body, time range).
- Diff two captures.
- Download a capture as JSON; download the raw body.
- Keyboard navigation.
- Light and dark theme, narrow and wide layouts.
- Shell-safe `curl` export (the old one broke on bodies containing a single quote).

## 7. Forwarding

The old application forwarded from the browser, which only works while a tab stays open and exposes the
target to cross-origin rules. webhooks.live forwards **on the server**:

| # | Behaviour |
|---|---|
| F1 | An endpoint may have forwarding rules: target URL, method override, content type override, header allow-list. |
| F2 | On each capture the server resends the request to the target asynchronously, after answering the sender. |
| F3 | The result (status, duration, error) is stored with the capture and shown in the interface. |
| F4 | Targets on private, loopback, and link-local addresses are refused unless an operator allows them (SSRF guard), re-checked after DNS resolution and on every redirect. |
| F5 | Manual re-send of any stored capture to a chosen target from the interface. |

Local-development forwarding (to `localhost` on the viewer's machine) is covered by a small CLI that
subscribes to the live stream and replays captures locally; it is a separate deliverable (see roadmap).

## 8. Access modes

| Mode | Who can create endpoints | Who can view |
|---|---|---|
| `open` | anyone | anyone holding the identifier |
| `sso` | signed-in users only | the creator and users the creator shares with; capture URLs stay public |
| `mixed` | anyone; signing in is optional (§13) | anonymous endpoints: anyone holding the identifier; owned endpoints: the owner and members; capture URLs stay public |

`sso` is OpenID Connect against the operator's provider (GlobalArt SSO in our deployment). The current
deployment fronts the interface with a gateway policy; webhooks.live should make that unnecessary by supporting
OIDC itself while keeping the capture path public.

## 9. Abuse control

| # | Behaviour |
|---|---|
| A1 | Per-IP rate limit on capture and on management calls, in-process, answering 429 with `Retry-After`. |
| A2 | Per-endpoint rate limit so one noisy sender cannot fill the store. |
| A3 | No shell-outs. The old application blocked abusive IPs by running a firewall command through `sudo`; webhooks.live does not do this. Blocking is the job of the gateway or firewall, fed by webhooks.live's metrics and logs. |
| A4 | Total storage cap with eviction of the least recently used endpoints. |

## 10. Operations

| # | Behaviour |
|---|---|
| O1 | One static binary plus a container image. Configuration by environment variables and an optional file. |
| O2 | Health and readiness endpoints; Prometheus metrics; structured JSON logs. |
| O3 | Graceful shutdown: in-flight captures finish, live streams close cleanly. |
| O4 | Horizontal scaling is possible: more than one replica behind a load balancer without sticky sessions (the old live-update server could not do this). |
| O5 | Data survives a restart. |

## 11. Out of scope

The first release left out scripting, workflow actions, email and DNS capture. They are now in scope as the
parity plan ([parity.md](parity.md), §13–§28). Still out of scope: billing and paid plans, a hosted service
spanning several operators, desktop and mobile apps. Raw TCP and WebSocket capture, custom domains, and teams
are deferred (§28).

## 12. Known defects of the old application not to be reproduced

- Whole request history of an endpoint stored in one Redis hash, fully loaded and sorted for every page.
- Live updates through a separate Node process, single replica only.
- Shell command with `sudo` for IP blocking.
- Forwarding performed by the browser.
- Unescaped `curl` export; HAR export that reads the wrong header.
- End-of-life runtime and framework versions; third-party analytics in the stock image.
- No search, no filtering, no authentication option.

## 13. Mixed mode, tiers and quotas

The public instance is open to everyone, yet several features need a person behind them. Mode `mixed`
combines `open` and `sso`.

| # | Behaviour |
|---|---|
| Q1 | In `mixed`, anyone creates endpoints without signing in, as in `open`; signing in is optional and adds ownership and higher limits. |
| Q2 | An anonymous endpoint (no owner) is reached by knowing its identifier. An owned endpoint is reached only by its owner, its members and their tokens. Capture URLs stay public for both. |
| Q3 | A signed-in user can claim an anonymous endpoint they can open; it becomes owned and private. Claiming an owned endpoint is refused. |
| Q4 | Features that need a person (tokens, aliases, schedules, never-expire, folders, templates, import, email and push notifications) are unavailable in `open` and to anonymous callers in `mixed`. |
| Q5 | Every caller has a tier: `anonymous` or `member` (signed in). The operator may define more tiers and assign them to users. |
| Q6 | Stored-data limits (captures per endpoint, body size, retention, storage) follow the owner's tier; anonymous endpoints use `anonymous`. |
| Q7 | Costly actions (outbound requests from rules, forwards, schedules and the composer; notifications; received email; DNS captures) are counted per owner for owned endpoints, and per endpoint and per creating network for anonymous ones. The counts hold across restarts and any number of replicas. |
| Q8 | A blocked action is reported, never silently dropped: the capture is still stored and the rule run or delivery shows `quota_exceeded`; API calls answer 429 with the limit's name. |
| Q9 | A caller can read their tier, limits and current usage. |

## 14. Capture kinds

| # | Behaviour |
|---|---|
| CK1 | A capture has a kind: `http`, `email`, `dns` or `schedule`. All kinds share the list, search, stream, delete, export and the per-endpoint cap. |
| CK2 | Non-HTTP kinds fill the common fields (method, URL, host, sender, headers, body) by a fixed mapping and keep their own data in a kind-specific part. |
| CK3 | The list filters by kind. |
| CK4 | Imported captures keep their kind and are marked as imported. |

## 15. Response templating

| # | Behaviour |
|---|---|
| T1 | When `response.templated` is on, the response status, content type, header values and body are templates. It is off by default, so `{{` in a static body stays literal. |
| T2 | Variables: method, path and path segments, query fields, header fields (case-insensitive), form fields, JSON body by JSON path, body text, sender IP, capture id and sequence, receive time, endpoint id and URL, rule variables, persistent variables and counters, results of earlier HTTP steps. |
| T3 | Generators: random UUID, random hex string, random integer in a range, current time in a chosen format. |
| T4 | Filters: default, upper, lower, trim, truncate, replace, length, JSON string escape, URL encode, HTML escape, base64 encode and decode, hex, SHA-256, HMAC-SHA-256, date format. |
| T5 | A missing variable renders empty unless a default is given; the run log notes it. |
| T6 | Templates read persistent counters; rules increment them atomically (R7). |
| T7 | Values are inserted as data and never evaluated again: a request containing `{{ ... }}` cannot inject template code. |
| T8 | Limits: source 64 KiB, 500 expressions, rendered body 1 MiB, rendered header value 1024 bytes, 50 ms per render. A limit hit or an invalid rendered status falls back to the static response; a rendered header that breaks the header rules (architecture §9) is dropped. Both are recorded. |
| T9 | Templates are checked on save; errors name the field, line and column. |

## 16. Rules

| # | Behaviour |
|---|---|
| R1 | An endpoint has an ordered list of rules (count limited by tier). A rule has a name, an enabled flag, a condition and ordered steps. |
| R2 | Conditions combine all, any and not over leaves. A leaf compares a variable (T2) using equals, not equals, contains, starts with, ends with, matches regex, exists, greater than, less than, in list, or IP in CIDR list. At most 4 levels and 32 leaves. An empty condition always matches. |
| R3 | Every enabled rule whose condition matches contributes its steps, in rule order. A `stop` step ends all later steps and rules. |
| R4 | Response steps: respond (status, content type, headers, body, all templated), set header, remove header, delay (added to the configured delay, total at most 10 s), stop. The last response step wins. |
| R5 | HTTP step: method, URL, headers and body templated; SSRF-guarded like forwarding; timeout up to 15 s; its result (status, headers, body up to 1 MiB, duration, error) is available to later steps by the step's name. |
| R6 | Extract: JSON path, or regex with a capture group, from a variable into a run variable. Transform: render a template into a run variable or into the payload later forward and HTTP steps send. |
| R7 | Persistent variables: set a variable kept on the endpoint (at most 50, 4 KiB each), or increment a counter atomically. |
| R8 | Side-effect steps: notify a channel (§27); forward through a forward rule or to a URL (the §7 queue); store the capture to an S3-compatible bucket. |
| R9 | Rules run after the capture is stored. Steps up to and including the last response step run before the response is sent; the remaining steps run after it, in order, on any replica, and survive restarts. |
| R10 | The part before the response has a budget of 3 s (operator-configurable up to 10 s), not counting delay. HTTP steps before the response are available to the `member` tier only. On an error or overrun the sender gets the static response with `X-Webhooks-Live-Rule-Error`, and the run records the cause. |
| R11 | After-response steps retry like forwards. A failing step stops the run unless it is marked continue on error. |
| R12 | Each run is stored with its capture: matched rules, each step's outcome, duration, error and outputs (truncated); it is shown in the capture detail and live. |
| R13 | Dry run: execute the rule set against a stored capture without side effects and read the log. Re-run: execute the after-response part again for a stored capture. Rules are edited in a visual editor (ui-design.md). |
| R14 | Secrets (tokens, passwords, keys) are stored per endpoint, write-only and encrypted; steps reference them by name. They are usable in outbound steps, notifications and schedules only, never in the response, and are masked in run logs. |

## 17. Scripts

| # | Behaviour |
|---|---|
| S1 | A script step runs a small sandboxed program (Rhai) with read-only access to the request and read-write access to run variables. Before the response it may set the response. |
| S2 | Scripts have no network, file, environment or clock access; outbound traffic goes through HTTP steps. |
| S3 | Per run: limited operations, call depth, string, array and map sizes, 32 MiB of memory, 100 ms before the response, 1 s after it. Any limit stops the script with a named error. |
| S4 | Deterministic: the clock is the capture time and random functions are seeded per run, so a re-run with the same inputs gives the same result. |
| S5 | Scripts are compiled on save; errors name line and column. |
| S6 | Printed lines go to the run log, up to 4 KiB. |
| S7 | Source up to 16 KiB. |

## 18. Email capture

| # | Behaviour |
|---|---|
| ML1 | When the operator sets a mail domain, an endpoint can enable the address `<id>@<mail domain>`; `<id>+<tag>@<mail domain>` also delivers and the tag is kept. |
| ML2 | Only enabled, live endpoints accept mail; other recipients are refused during the SMTP dialogue, so no bounce is ever sent later. |
| ML3 | A message becomes an `email` capture: envelope sender and recipients, client address and greeting name, whether TLS was used, all headers, subject, from, to, cc, date, message id, the raw message as body, and its MIME parts (text, HTML, attachments with file name, type and size). |
| ML4 | Each part can be downloaded decoded, with the same safe headers as raw bodies; HTML is never served as HTML. |
| ML5 | Message size limit 10 MiB (tier); bigger messages are refused with the SMTP size error. |
| ML6 | Limits per client address and per endpoint per hour (tier); over a limit the message is refused with a temporary error. |
| ML7 | Rules run on email captures (after-response steps only); forwarding sends the raw message as `message/rfc822`. |
| ML8 | No spam filtering: the catcher shows what arrived. Sender authentication results (SPF, DKIM, DMARC) are deferred (§28). |
| ML9 | Email captures appear in lists and the live stream like any other capture. |

## 19. DNS capture

| # | Behaviour |
|---|---|
| D1 | When the operator sets a DNS zone, an endpoint can enable the name `<id>.<zone>`; queries for it and any name below it are captured. |
| D2 | Stored: query name as received, type, class, transport (UDP or TCP), resolver address, EDNS client subnet when present, flags, time. |
| D3 | Answers: A and AAAA with operator-set addresses (or none), TXT with the capture id, other types empty. Unknown or disabled names answer NXDOMAIN and are not stored. |
| D4 | The zone apex answers SOA and NS. The server is authoritative for its zone only and never recurses. |
| D5 | Matching ignores case (resolvers randomise it); the original name is stored. |
| D6 | Answers stay small; `ANY` gets a minimal answer; responses are rate-limited per source network; captures are rate-limited per endpoint (tier). Queries over the capture rate are answered but not stored. |
| D7 | Rules run on DNS captures (after-response steps only). |
| D8 | The endpoint shows its DNS name. |

## 20. Endpoint management additions

| # | Behaviour |
|---|---|
| E7 | Tags: up to 10 per endpoint, set by editors, used to filter the endpoint list. |
| E8 | Folders: a signed-in user's private grouping of endpoints they can see; an endpoint may sit in several folders. |
| E9 | Retention per endpoint: a sliding window from 1 hour to the tier maximum. Signed-in owners may mark up to a tier number of endpoints never-expire. |
| E10 | Capture TTL: captures older than a set age are deleted automatically, independent of endpoint expiry. |
| E11 | Per-endpoint capture cap, set by the owner up to the tier maximum. |
| E12 | Aliases: a signed-in owner gives an endpoint up to a tier number of aliases; `/a/<alias>` and its sub-paths behave exactly like the capture URL. |
| E13 | An alias is 3–23 characters of lowercase letters, digits and hyphens, starting with a letter and not ending with a hyphen; reserved names are refused; aliases are unique per instance; a released alias cannot be taken by anyone else for 30 days. |
| E14 | Sender allow list: up to 20 CIDR ranges; other senders get 403 and nothing is stored. |
| E15 | Capture protection: HTTP Basic credentials or a named header with a secret value. Failures get 401 without a browser login prompt; nothing is stored. |
| E16 | View password for anonymous endpoints: reading needs an unlock that issues a grant valid for 12 h; unlock attempts are rate-limited. |
| E17 | Clone: copy the configuration (response, forward rules, rules, notification channels, schedules as disabled, secrets when the caller owns both) into a new endpoint. Captures are never copied. |
| E18 | Templates: save an endpoint's configuration as a named private template (signed in) and create endpoints from it. |
| E19 | Signature verification: per endpoint, a scheme (generic HMAC with header, algorithm and encoding, or a preset for a common provider) and a secret. Each HTTP capture records `valid`, `invalid` or `missing`, shown in the list and available to rules. |
| E20 | Subdomain URL: `https://<id>.<capture host>/...` captures like `/<id>/...` when the operator configures a wildcard host. |

## 21. Viewing additions

| # | Behaviour |
|---|---|
| V7 | Combined inbox: one list across several endpoints (signed in: all own and shared endpoints, or a folder; anonymous: up to 20 named identifiers), newest first, with the §5 filters. |
| V8 | A combined live stream for the same set. |
| V9 | Notes (up to 2000 characters) and stars on captures; ring mode never removes a starred capture. |
| V10 | Stats: counts per hour or day over the retained captures, split by method, kind and response status class. |

## 22. API access

| # | Behaviour |
|---|---|
| K1 | A signed-in user creates personal API tokens with a name, scopes and an expiry of up to 365 days. The token is shown once. |
| K2 | Scopes: read, write endpoints, write captures, write rules, send requests; a token may be limited to listed endpoints. |
| K3 | A token never exceeds its user's role; tokens cannot manage tokens, members or ownership. |
| K4 | Tokens are stored hashed. The list shows name, prefix, scopes, creation, last use and expiry; revoking takes effect at once. |
| K5 | `Authorization: Bearer <token>` works on every `/api/v1` route, the live streams included. |
| K6 | Expired, revoked or unknown tokens answer 401; a missing scope answers 403. |
| K7 | The server serves an OpenAPI 3.1 document of the whole API. |
| K8 | The interface has an API reference page built from that document. |
| K9 | Browser code on other origins may call the API with a token (CORS without credentials); cookie sessions stay same-origin. |

## 23. Command-line forwarder

| # | Behaviour |
|---|---|
| L1 | A command-line program forwards the captures of one endpoint to a local URL as they arrive. |
| L2 | It authenticates with an API token; anonymous endpoints need none. |
| L3 | It replays method, sub-path, query, headers (without hop-by-hop ones) and body; `Host` is the target's unless asked otherwise. |
| L4 | It resumes after a disconnect or restart from the last forwarded sequence number, without gaps. |
| L5 | It reconnects with backoff and prints each forward: capture, local status, duration. |
| L6 | Options: start now or at a sequence number, kinds to forward, path prefix, concurrency (default 1, in order). |
| L7 | Binaries for Linux, macOS and Windows. |
| L8 | Local targets are not restricted: the program runs on the user's own machine. |

## 24. Composer and replay

| # | Behaviour |
|---|---|
| P1 | Compose a request (method, URL, headers, body) in the interface and send it through the server. |
| P2 | Same SSRF rules as forwarding. |
| P3 | Result: status, headers, first 1 MiB of body, duration, final URL when redirects are followed, error. |
| P4 | Any capture can be opened in the composer for an edited replay; exact resend stays F5. |
| P5 | A request to the instance's own capture URL is handled in-process and appears as a capture. |
| P6 | History: the last 50 sends per user (per endpoint for anonymous callers); rate-limited by tier. |

## 25. Scheduled requests

| # | Behaviour |
|---|---|
| SC1 | A signed-in owner adds schedules to an endpoint: name, cron expression (5 fields), time zone, request template (method, URL, headers, body), enabled. |
| SC2 | Each run's response becomes a `schedule` capture on the endpoint: status, headers, first 1 MiB of body, duration, error. |
| SC3 | Minimum interval and number of schedules per user follow the tier (default 5 minutes, 5 schedules). |
| SC4 | SSRF-guarded like forwarding; secrets usable in URL and headers. |
| SC5 | A schedule pauses itself after 10 consecutive failures and shows why. |
| SC6 | Runs missed during downtime are not backfilled. |
| SC7 | Run now. |
| SC8 | Rules run on schedule captures (after-response steps only). |

## 26. Export and import

| # | Behaviour |
|---|---|
| X1 | Export all captures of an endpoint, a filtered set (§5 filters) or a selection. |
| X2 | Formats: JSON document, NDJSON, CSV (summary and body preview), ZIP (NDJSON index plus raw bodies). |
| X3 | Exports stream; server memory does not grow with export size. |
| X4 | CSV cells that a spreadsheet would read as formulas are neutralised. |
| X5 | Export size and exports per hour are limited by tier. |
| X6 | Exports never contain secrets, capture-protection credentials or view passwords. |
| X7 | Signed-in owners import a JSON or NDJSON export into an endpoint. |
| X8 | Imported captures keep kind, time, method, headers and body, get new ids, are marked imported and count against limits; rules and forwards do not run on them. |
| X9 | Import stops at the endpoint's capacity and reports the numbers imported and skipped. |

## 27. Notifications

| # | Behaviour |
|---|---|
| N1 | Per-endpoint channels: generic webhook, Slack, Discord, Telegram, email, browser push. |
| N2 | Trigger: every capture (optionally only those matching an R2 condition), or only rule notify steps. |
| N3 | The message is a template; the default names the endpoint, kind, method, path and links to the capture; no body by default. |
| N4 | Rate limits per channel and per owner (tier). Notifications over a limit are counted, and one summary goes out when the window ends. |
| N5 | Slack, Discord and Telegram channels call only those vendors' fixed API hosts; the generic webhook is SSRF-guarded. |
| N6 | Chat messages cannot mass-mention: channel-wide mentions are neutralised. |
| N7 | Delivery is queued and retried; the result is shown on the capture. |
| N8 | A test action sends one sample notification. |
| N9 | Email goes only to addresses the user verified by link, carries a one-click disable link, and has a daily cap per user. |
| N10 | Browser push reaches the signed-in user's subscribed browsers; the payload carries the summary only. |

## 28. Other expectations

| Item | Decision | Reason |
|---|---|---|
| Search by body text | Done (V1, `q`); extended to the combined inbox (V7) | |
| Field-qualified search (header only, JSON path) | Deferred | Substring search covers most needs; JSON-path search parses every row |
| Retention controls | In: E9–E11 | |
| Payload size limit per tier | In: Q6 | |
| Webhook signature helpers | In: E19 | |
| Replay history | In: forward attempts (F3) and composer history (P6) | |
| Response header rewriting | In: R4 | |
| Binary bodies | Done: base64 in JSON, raw download | |
| Multi-endpoint dashboard | In: V7, V8, V10 | |
| gRPC and HTTP/2 senders | Done: captured as HTTP with a binary body | |
| Periodic checks of a URL | In: SC1 | Same mechanism as schedules |
| Mock API per path and method | In: rules with path and method conditions | No separate feature needed |
| Simulated failures (every Nth call fails, random latency) | In: counters, conditions, delay | |
| SPF, DKIM, DMARC results on email | Deferred | DNS lookups per message widen the abuse surface |
| Raw TCP capture | Deferred | A port per endpoint and new exposure |
| WebSocket message capture | Deferred | Long-lived connections on the capture path |
| Custom domains | Deferred | Certificate issuance per domain |
| Teams and organisations | Deferred | Per-endpoint sharing and folders cover the first need |
| Spreadsheet export target | Rejected | Third-party OAuth; CSV export and the HTTP step cover it |
| Sending email as the user from rules | Rejected | Spam relay |
| Billing and paid plans | Rejected | The owner's separate product decision |
| Desktop and mobile apps | Rejected | The interface is responsive; the CLI covers local use |
