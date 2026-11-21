# webhooks.live parity plan

Status: draft 1, 2026-11-21.

Goal: full functional parity with the commercial hosted request catcher the owner uses as the benchmark.
Functionality is reimplemented under webhooks.live's own names, API, data model and look; no text, UI,
product term or documentation of that service is copied ([licensing.md](licensing.md)). Behaviour lives in
[product-spec.md](product-spec.md) §13–§28, design in [architecture.md](architecture.md) §16–§36, the HTTP
contract in [api.md](api.md) §9–§24, the interface in [ui-design.md](ui-design.md).

Status: **done** merged and live on webhooks.live; **in progress** partly merged or its milestone is running;
**planned** designed, not started.

## 1. Matrix

| # | Capability | Status now | Specified in | Milestone |
|---|---|---|---|---|
| 1 | Capture any HTTP method on `/{id}` and sub-paths | done | spec C1, C7 | M4 |
| 2 | Configured static response: status, content type, body, up to 20 headers | done | spec C2, E1 | M5 |
| 3 | Status override by sub-path (`/{id}/404`) | done | spec C3 | M5 |
| 4 | Response delay up to 10 s | done | spec C5 | M5 |
| 5 | CORS mode and preflight answers | done | spec C6 | M5 |
| 6 | Real sender IP behind trusted proxies | done | spec C8 | M4 |
| 7 | Live stream across replicas, resumable | done | spec V4, arch §3 | M7 |
| 8 | List, detail, latest, raw body, delete, clear | done | spec V1–V3 | M6 |
| 9 | Search by text in path, query, headers, body; filters by method, status, time | done (API), in progress (UI) | spec §6, api §5.1 | M6, F9 |
| 10 | Body viewer, JSON/XML pretty print, binary as base64 and raw download | in progress | spec §6, ui-design | F5–F6 |
| 11 | Copy as curl and HAR, download capture as JSON | in progress | spec §6 | F5–F6 |
| 12 | Diff of two captures | in progress | spec §6 | F9 |
| 13 | Server-side forwarding rules with durable queue, retries, results | done | spec F1–F4, arch §6 | M10, M11 |
| 14 | SSRF-guarded outbound client (every hop, every resolve) | done | arch §6 | M10 |
| 15 | Manual resend of a capture | done | spec F5 | M11 |
| 16 | Per-IP and per-endpoint rate limits, capture cap, storage cap | in progress (M9 fixes) | spec A1–A4, arch §8 | M9 |
| 17 | OIDC sign-in | done | arch §7 | M8 |
| 18 | Ownership, roles on routes, sharing with viewer and editor | in progress | spec §8, arch §7 | M8 |
| 19 | Mixed mode: anonymous use plus optional sign-in on one instance | planned | spec Q1–Q4 | M17 |
| 20 | Identity tiers and quotas for costly features | planned | spec Q5–Q9, arch §17 | M18 |
| 21 | Capture kinds (HTTP, email, DNS, scheduled) in one list and stream | planned | spec CK1–CK4, arch §20 | M16 |
| 22 | Response templating from request data, random ids, counters | planned | spec T1–T9, arch §21 | M28, M29 |
| 23 | Persistent endpoint variables and counters | planned | spec T6, R7 | M29 |
| 24 | Rules: conditions over header, query, JSON path, regex, method, IP, path | planned | spec R1–R3 | M30, M31 |
| 25 | Rule steps that shape the response (respond, header, delay, stop) | planned | spec R4 | M31 |
| 26 | Rule step: outbound HTTP request, result usable by later steps | planned | spec R5 | M32 |
| 27 | Rule steps: extract (JSON path, regex), set variable, transform payload | planned | spec R6, R7 | M31 |
| 28 | Rule steps: notify, forward, store elsewhere (HTTP, S3-compatible) | planned | spec R8 | M32, M35 |
| 29 | Rule run log per capture, re-run, dry run against a stored capture | planned | spec R9–R12 | M31 |
| 30 | Visual rule editor | planned | spec R13, ui-design | UI track |
| 31 | Sandboxed scripting for advanced cases | planned | spec S1–S7, arch §23 | M33 |
| 32 | Webhook signature verification helpers | planned | spec E19 | M34 |
| 33 | Email address per endpoint, messages captured with parts and attachments | planned | spec ML1–ML9, arch §28 | M44, M45 |
| 34 | DNS name per endpoint, queries captured | planned | spec D1–D8, arch §29 | M46, M47 |
| 35 | Subdomain capture URL (`{id}.<host>`) | planned | spec E20 | M48 |
| 36 | Labels (tags) and folders | planned | spec E7, E8 | M22 |
| 37 | Combined inbox and stream across endpoints | planned | spec V7, V8 | M42 |
| 38 | Per-endpoint stats (counts over time, by method and kind) | planned | spec V10 | M22 |
| 39 | Capture notes and stars (starred captures survive ring eviction) | planned | spec V9 | M22 |
| 40 | Custom retention, never-expire for signed-in owners, capture TTL, per-endpoint cap | planned | spec E9–E11 | M23 |
| 41 | URL aliases | planned | spec E12, E13 | M24 |
| 42 | Sender IP allow list | planned | spec E14 | M25 |
| 43 | Capture URL protection: HTTP Basic or shared secret header | planned | spec E15 | M25 |
| 44 | View password for anonymous endpoints | planned | spec E16 | M26 |
| 45 | Clone an endpoint, endpoint templates | planned | spec E17, E18 | M27 |
| 46 | Personal API tokens with scopes, expiry, revocation | planned | spec K1–K6, arch §19 | M20 |
| 47 | OpenAPI document served by the app, API reference page | planned | spec K7, K8 | M21 |
| 48 | CORS on the API for token clients | planned | spec K9 | M20 |
| 49 | Command-line forwarder to localhost with resume | planned | spec L1–L8, arch §31 | M43 |
| 50 | Request composer through the server, send history | planned | spec P1–P6 | M38 |
| 51 | Replay a capture as is (existing resend) or edited (composer) | done (as is), planned (edited) | spec F5, P4 | M11, M38 |
| 52 | Scheduled requests, results stored as captures | planned | spec SC1–SC8 | M39 |
| 53 | Export JSON, NDJSON, CSV, ZIP with bodies, streaming | planned | spec X1–X6 | M40 |
| 54 | Import of a JSON export | planned | spec X7–X9 | M41 |
| 55 | Notifications: webhook, Slack, Discord, Telegram | planned | spec N1–N8 | M35 |
| 56 | Notifications: email to verified addresses | planned | spec N9 | M36 |
| 57 | Notifications: browser push | planned | spec N10 | M37 |
| 58 | Secrets for rule and notification configs, encrypted at rest | planned | spec R14, arch §18 | M19 |

## 2. Deliberate differences

| Area | webhooks.live | Why |
|---|---|---|
| Scripting | Rhai, a small Rust-embedded language, no network or I/O inside scripts; network only through rule steps | One SSRF-guarded path for all outbound traffic; hard CPU, memory and time limits (arch §23) |
| Templates | `{{ path \| filter }}` substitution, no loops or conditionals; logic lives in rules and scripts | Output size is linear in input, no template injection by construction (arch §21) |
| Captured HTML and email HTML | Shown as text, never rendered on the app origin | Stored XSS stays impossible (arch §9) |
| Capture auth failure | 401 without `WWW-Authenticate` | No native browser login prompt on the shared origin (arch §9) |
| Notifications | Email only to verified addresses; chat channels pinned to vendor hosts | The service cannot be used as a spam or SSRF relay |
| Billing and paid plans | None; tiers are anonymous and signed-in, set by the operator | Self-hosted product; billing is the owner's separate decision |
| Hosted scale | One instance per operator, Postgres only | Product scope (spec §11) |
| Spreadsheet export target | Not built; CSV export and the generic HTTP step cover it | OAuth to a third-party office suite is out of scope |
| Raw TCP capture, WebSocket message capture, custom domains | Deferred | Port allocation, per-domain TLS; see spec §28 |
