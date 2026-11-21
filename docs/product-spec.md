# webhooks.live product specification

Status: draft 1, 2026-11-21.

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

## 11. Out of scope for the first release

Scripting and visual workflow actions, email receiving, TCP/DNS capture, user teams, billing, a hosted
multi-tenant service, and desktop or mobile apps.

## 12. Known defects of the old application not to be reproduced

- Whole request history of an endpoint stored in one Redis hash, fully loaded and sorted for every page.
- Live updates through a separate Node process, single replica only.
- Shell command with `sudo` for IP blocking.
- Forwarding performed by the browser.
- Unescaped `curl` export; HAR export that reads the wrong header.
- End-of-life runtime and framework versions; third-party analytics in the stock image.
- No search, no filtering, no authentication option.
