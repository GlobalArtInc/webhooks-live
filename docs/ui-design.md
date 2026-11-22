# webhooks.live interface design

Status: draft 2, 2026-11-21; sections 13 to 31 add full functional parity. Requirements: [product-spec.md](product-spec.md), sections 6 and 7 above all.
This document fixes the web interface: look, screens, behaviour, tokens, frontend stack, and milestones.
Data needs are stated per screen (section 9); wire formats belong to the API documents.

## 1. Design direction

webhooks.live is used by developers wiring up an integration: they create an endpoint, paste the URL into a
provider, trigger an event, and read what arrived. The tab sits in the background for long stretches and
is glanced at when something should have happened. In the first five seconds the screen must answer three
questions: what is my capture URL (one click to copy), did anything arrive (count, newest on top, live
indicator), and what exactly arrived (method, path, body without a click when following).

Personality: a live wire, not a dashboard. The identity is the stream itself: requests landing in real
time, a signal light that says the line is open. Dense rows, monospace for anything that came off the wire,
sans for chrome, one warm "signal amber" accent (an on-air lamp) on cool ink neutrals, no hero art, no
gradients. Dark and light themes are equal citizens; the default follows the system.

| Kept from the old interface (what made it fast) | Changed |
|---|---|
| List left, detail right, newest first, one click to select | No top navigation bar; the capture URL strip is the page header |
| Copy URL one click away | Amber on ink instead of a green bar and Bootstrap grey; flat surfaces, hairline borders |
| Auto-jump to newest | Follow mode that pauses itself when the user picks an older capture |
| Unread counter in the title, toast | Favicon badge, "N new" pill, polite screen-reader announcement |
| Pretty JSON/XML with raw toggle | Sectioned detail with sticky section nav, lazy code viewer for large bodies |
| All on one page | Configuration in a deep-linkable endpoint workspace next to the live list (section 15) |

**phoenix-ui: webhooks.live stands alone.** phoenix-ui (`globalart/platform/frontend/libs/phoenix-ui`) is an
Angular 19 component library; none of its components run in React, and the package is `UNLICENSED`, which
the frontend dependency policy in [licensing.md](licensing.md) forbids. Family resemblance is kept only
where it is free: the IBM Plex Sans and Plex Mono pair and the Lucide icon set (`lucide-react`), both of
which phoenix-ui uses. Rejected: importing `phoenix-ui.css` tokens, because its blue admin palette and
12 px text tokens would fight this product's own rules and still tie an independently licensable product to a
private package.

Brand mark: the display name `webhooks.live` set as a wordmark in Plex Mono 600, all lowercase, with the
dot drawn as a filled amber disc, the "live" light. In the app header that disc mirrors the stream (solid
amber when live, hollow `text-muted` ring when offline); the text `ConnectionIndicator` stays the accessible
carrier. Favicon: the disc followed by three stacked bars of decreasing length, rows arriving; with unread
captures the disc grows into the count badge. Technical identifiers use the slug `webhooks-live`.

## 2. Information architecture and routes

Routes are relative to the interface base path, which the backend chooses so that it can never be read as
an endpoint identifier. Filters live in the query string so a filtered view is a shareable link (V6).

| Route | Screen | Notes |
|---|---|---|
| `/` | Home / create | Create button, custom-response disclosure, my endpoints |
| `/e/:endpointId` | Endpoint view | Main screen; nothing selected means the newest is selected on wide/medium |
| `/e/:endpointId/c/:captureId` | Endpoint view, capture selected | Narrow: detail full screen |
| `/e/:endpointId/settings` | Endpoint workspace, General section (section 15) | Sections at `/e/:endpointId/settings/:section` (section 14) |
| `/e/:endpointId/forwarding` | Redirect to `/e/:endpointId/settings/forwarding` | Kept so old links work |
| `/e/:endpointId/diff/:a/:b` | Diff of two captures | Replaces the detail pane; narrow: full screen |
| `/endpoints` | My endpoints | Narrow only as a page; wide shows them in the rail |
| `/signin` | Sign-in (sso mode) | `?next=` returns to the original route |
| Filter parameters (section 23) | Filter state on endpoint and inbox routes | `order=oldest` flips V1 sort |

| State | Trigger | Shows |
|---|---|---|
| Empty endpoint | Endpoint exists, zero captures | Capture URL large, copyable sample `curl`, "Waiting for the first request" with a slow pulse, response config summary |
| Gone (410) | `endpoint_gone`: endpoint unknown, expired or deleted, or in sso not shared with the signed-in user | Full page: "This endpoint no longer exists" (sso adds "or it is not shared with you"), Create new, Remove from my endpoints; no list chrome |
| Not shared (403 `forbidden`, sso) | An action the caller's role does not allow, e.g. a viewer editing settings or an editor deleting the endpoint | Inline message on that action: "Your role does not allow this; ask the owner"; the view stays |
| Sign-in (sso) | No session on any non-capture route | Wordmark, one button "Sign in with {auth.provider_name}", or "Sign in" when it is null; nothing else |
| Error | Initial load fails (5xx, malformed answer) | Inline panel with status, request id when known, Retry; chrome stays |
| Offline | `navigator.onLine` false or stream down past 3 retries | Amber banner under the header; list and cached details stay readable; actions that write are disabled with a reason tooltip |
| Not found | Unknown interface route | Short page with link home |
| Capture gone | Selected capture deleted elsewhere | Detail shows "Deleted" placeholder; selection moves only on user action |

## 3. Endpoint view layouts

| Breakpoint | Width | Regions |
|---|---|---|
| Wide | >= 1200 px | Rail 232 px (collapsible to 56 px) / list 380 px (resizable 300-560) / detail |
| Medium | 768-1199 px | List 320 px / detail; rail becomes a left drawer; filter bar collapses to search plus "Filters (n)" popover |
| Narrow | < 768 px, designed at 360 | One column: header, filter chips, list; detail, settings, forwarding, diff are full-screen routes |

Wide (1440 px):

```
+--------------+-------------------------------------------------------------------------------------------+
| webhooks.live|  https://hooks.example/k7Qm2xTa9vNc  [Copy] [Copy curl]    312/500  expires in 6d          |
|              |                                     (*) Live   [Follow: on]   [Settings] [Forwarding] [?] |
| MY ENDPOINTS +---------------------------------------+---------------------------------------------------+
| > stripe-dev | [/ Search headers and body        ]   | POST  /k7Qm2xTa9vNc/stripe?mode=test     [Copy v] |
|   github-app | [GET] [POST] [2xx] [4xx] [Last 1h] x  | 203.0.113.7 . 21 Nov 14:02:11 (12 s) . 1.4 KiB    |
|   shop-test  +---------------------------------------+ application/json . answered 200 . c_8f2kQ        |
|              |          ^ 2 new                      | Forward  https://api.dev/hook  200  143 ms [Resend]|
|              | * POST    /stripe          1.4K  12s  +---------------------------------------------------+
|              | * POST    /stripe          1.2K   1m  | Summary  Forward  Query 1  Body  Headers 18  Raw  |
|              | > GET     /health?x=1       0 B   5m  +---------------------------------------------------+
|              |   PUT     /orders/77  404  88 B  12m  | BODY  JSON  [Pretty|Raw] [Wrap] [Download]        |
|              |   DELETE  /orders/77       0 B  13m  | 1  {                                              |
|              |   OPTIONS /orders           0 B  13m  | 2    "id": "evt_1Q",                              |
|              |   ...                                 | 3    "type": "payment_intent.succeeded",          |
| [+ New]      |                                       | 4 >  "data": { ... }                              |
| [Theme] [?]  | |< First  < Prev  Next >  Last >|  [Delete all] | HEADERS                       [Show all 18] |
+--------------+---------------------------------------+---------------------------------------------------+
```

`*` is the unread dot, `>` the selected row (amber left bar plus soft amber fill), `404` an override chip.

Medium (1024 px): the rail is behind `[=]`; filters fold into a popover.

```
+--------------------------------------------------------------------------------------------+
| [=] webhooks.live  https://hooks.example/k7Qm2xTa9vNc [Copy]   (*) Live  [Follow]  [...]    |
+-----------------------------------+--------------------------------------------------------+
| [/ Search          ] [Filters 2]  | POST /k7Qm2xTa9vNc/stripe?mode=test          [Copy v]  |
| * POST    /stripe     1.4K   12s  | 203.0.113.7 . 14:02:11 (12 s) . 1.4 KiB                |
| > GET     /health?x=1  0 B    5m  | Summary  Forward  Query  Body  Headers                 |
|   PUT     /orders/77  404    12m  | { "id": "evt_1Q", ...                                  |
| |< < > >|            [Delete all] |                                                        |
+-----------------------------------+--------------------------------------------------------+
```

Narrow (360 px), list first; the capture URL stays visible because it is the most used action:

```
+------------------------------------+    +------------------------------------+
| [=] webhooks.live  (*) Live  [...] |    | [<] POST /stripe          [Copy v] |
| k7Qm2x...vNc             [Copy]    |    | 14:02:11 (12 s) . 1.4 KiB . 200    |
| [/ Search      ] [Filters 2]       |    | [Summary][Body][Headers 18][Query] |
| [Follow: off]           ^ 2 new    |    | {                                  |
| * POST /stripe                12s  |    |   "id": "evt_1Q",                  |
|   1.4 KiB . 203.0.113.7            |    |   "type": "payment_intent..."      |
|   GET  /health?x=1             5m  |    | }                                  |
|   0 B . 198.51.100.4               |    |                                    |
|   PUT  /orders/77  404        12m  |    |                                    |
+------------------------------------+    | [< Newer]   3 / 312    [Older >]   |
                                          +------------------------------------+
```

| Element | Wide | Medium | Narrow |
|---|---|---|---|
| My endpoints | Rail | Left drawer | `[=]` drawer and `/endpoints` |
| Capture URL | Full, header | Full, header | Middle-truncated id, Copy button |
| Filters | Popover from `Filters (n)` next to the search (the inline chip block took a third of the list height) | Popover | Bottom sheet |
| Expiry and capacity | Header text | `[...]` menu | `[...]` menu |
| Configuration | Workspace route, side section nav (section 15) | Workspace route, section tabs | Section list, then full-screen section |
| Pager | List footer | List footer | Detail footer (newer/older) |
| Detail sections | Sticky section nav | Sticky section nav | Sticky segmented tabs (scroll-linked) |
| Row height | 36 px, one line | 36 px | 56 px, two lines |

## 4. Home / create

```
+----------------------------------------------------------------------------------+
|  webhooks.live                                                     [Theme] [?]  |
|                                                                                  |
|      Watch HTTP requests arrive live.                                            |
|      [ Create endpoint  (Enter) ]                                                |
|      > Custom response                                                           |
|        Status [200 v]  Content type [text/plain      v]  Delay [0 s ----o---]    |
|        Body  [                                                    ]  [ ] CORS    |
|                                                                                  |
|      MY ENDPOINTS                                                                |
|      stripe-dev   k7Qm2x...vNc   312 captures   last 12 s ago        [Open] [x]  |
|      github-app   Hq83Lw...0aZ     4 captures   last 2 d ago         [Open] [x]  |
+----------------------------------------------------------------------------------+
```

Decision: no endpoint is created by merely visiting `/`; one press of Enter creates it and opens the
empty state. Rejected: auto-create on visit (the old behaviour), because link previews and crawlers would
create junk endpoints and eat the storage cap (A4). Labels for endpoints are local and editable inline.

## 5. Capture detail

Decision: one scrolling column of sections with a sticky section nav, not tabs. The common task is reading
a signature header next to the body; tabs hide one while showing the other. Rejected: tabs (old-style
panes), for that reason. Narrow uses the same sections with the nav rendered as scroll-linked tabs.

| Section | Shown when | Content |
|---|---|---|
| Summary | Always | Method badge, full URL (mono, wraps, copy), sender IP, absolute and relative time, size, content type, capture id, answered status, override chip |
| Forward | Capture has forward results | One row per attempt: target, status chip, duration, error text, time; `Resend` |
| Query | Query not empty | Key-value table, multi-values as repeated rows, decoded with a raw toggle |
| Form | Form content type | Fields table; multipart files as name, filename, type, size, `Download` |
| Body | Body not empty | Body viewer (below) |
| Headers | Always | Key-value table in original order, multi-valued kept; first 12 rows then `Show all n`; per-row copy |

Body viewer:

| Case | Behaviour |
|---|---|
| JSON (`application/json`, `+json`, or sniffed `{`/`[` that parses) | Pretty by default, 2-space indent, folding, syntax colours; parse error shows raw with "Not valid JSON at 3:14" |
| XML (`xml`, `+xml`) | Pretty-indented; raw toggle |
| Text | Raw, wrap toggle |
| Large (> 1 MiB, or announced in summary form per V5) | Fetched on open; first 256 KiB shown raw, `Load full body (4.2 MiB)`; pretty-print runs in a Web Worker and is never automatic above 1 MiB |
| Binary (NUL bytes or invalid UTF-8 in the first 8 KiB) | Type, size, SHA-256 (Web Crypto), hex view of the first 4 KiB, `Download raw` |
| Image (`png`, `jpeg`, `gif`, `webp` only) | Preview from a blob URL in `<img>`; SVG is always shown as text |
| HTML | Text with highlighting; never rendered |

Viewer controls: `[Pretty|Raw]`, `[Wrap]`, in-body search, `[Download]`. Pretty/raw and wrap are
remembered preferences.

Copy-as menu (`[Copy v]`, `y` copies curl directly):

| Item | Output |
|---|---|
| curl | POSIX shell, every value single-quoted with `'\''` escaping; always `--globoff`, `--path-as-is` and the URL passed with `--url`; `HEAD` uses `--head`. Text bodies under 64 KiB: `--data-raw '<body>'` (never `--data-binary`, which would read a body that starts with `@` as a file name). Binary bodies, text with control characters, and text of 64 KiB or more: `printf '<octal escapes, printable ASCII kept literal>' \| curl ... --data-binary @-` (POSIX `\ooo`, `%` as `%%`), self-contained. Bodies above 256 KiB: `--data-binary @capture-<short id>.bin` plus a Download raw button. Control characters in header values or the URL are written as `"$(printf '\ooo')"` words; a header with more than 8 of them is skipped and reported. Headers curl sets itself, hop-by-hop headers and the headers the edge proxy adds (`x-forwarded-*`, `x-envoy-*`, `x-request-id`, `x-endpoint-id`) are omitted, and stay in the headers table |
| HAR | HAR 1.2 log with one entry built from the capture; the response block carries the answered status and the endpoint's current response configuration, which may differ from what was served at capture time |
| JSON | Full capture as JSON |
| URL | Captured URL |
| Download JSON / Download raw body / Download HAR | Same content as files |

Exports are pure client-side functions over the full capture. Rejected: server-rendered exports, because
the client already holds the data and pure functions are trivially unit-tested for the quoting defect
listed in spec section 12. Exporting many captures at once is a server job (section 21).

Diff (`/e/:id/diff/:a/:b`; mark A with `d`, then `d` on B, or row menu "Compare with marked"):

```
+--------------------------------------------------------------------------------------------+
| A  POST /stripe  14:02:11       vs   B  POST /stripe  13:59:40     [Swap] [Hide volatile x] |
| REQUEST LINE   same method . path differs: /stripe?mode=test | /stripe                       |
| HEADERS        3 changed . 1 added . 14 same (hidden)                                        |
|   stripe-signature   t=1732.. v1=9a..        | t=1732.. v1=41..                 changed      |
|   x-retry            -                       | 1                                added        |
| BODY (JSON, keys sorted)                                                                     |
|   12    "amount": 2000,                      |  12    "amount": 2500,                         |
+--------------------------------------------------------------------------------------------+
```

Side-by-side at wide and medium, unified at narrow. JSON bodies are normalised (sorted keys, pretty)
before diffing; binary bodies compare size and SHA-256 only. "Hide volatile" (default on) hides `date`,
`content-length`, `x-request-id`, and trace headers; the list is a preference.

Settings (workspace sections General, Response and Access, section 15): response status, content type, body (small code editor), delay slider 0-10 s, CORS toggle,
ring mode toggle when the server allows it, capacity meter, expiry ("sliding, 6 days of inactivity
left"), local label; sso mode adds "Shared with" (add/remove users); danger zone: Delete all captures,
Delete endpoint, each behind a confirm dialog.

Forwarding (workspace section, section 15): rule list with enabled toggle; rule editor with target URL, method override, content
type override, header allow-list (chips); `Test with latest capture`. SSRF refusals (F4) show as field
errors on the target with the server's reason. Resend dialog (F5): target prefilled from rules or free
text, method override, result appears in the Forward section when the server reports it.

## 6. Live behaviour

| Topic | Rule |
|---|---|
| Arrival | Row inserted at its sorted position; background fades from accent-soft to transparent over 1600 ms; unread dot stays until the row is selected |
| No jumping | Selection is held by capture id, never by index. If the list is scrolled away from the top, `scrollTop` is shifted by the inserted height so visible rows do not move, and a `^ n new` pill appears; clicking it scrolls to top |
| No focus theft | Arrivals never move keyboard focus, never open sheets, never close menus |
| Follow mode | Off by default (preference). On: each arrival becomes the selection, the detail updates, focus stays put. Picking any non-newest capture turns follow off with a toast "Follow paused"; `Shift+K` or the toggle turns it on |
| Toast | Shown on arrival only when the new row is not visible (scrolled away, narrow detail open, follow off); one toast aggregates bursts; preference to disable |
| Filters and arrivals | Method, status, and time filters are applied to live summaries client-side; non-matching arrivals add to "n new hidden by filters". With a text query active, an arrival triggers a debounced (1 s) refetch of the first page |
| Title | `(3) stripe-dev . webhooks.live`; counts arrivals since the tab became hidden; cleared on `visibilitychange` to visible |
| Favicon | Stream glyph whose amber disc becomes a badge with the count (1-9, then `9+`), drawn on a 32 px canvas; restored when cleared |
| Connection indicator | `Live` (success dot), `Connecting` (warning dot, pulse), `Offline` (danger dot) with "last event 2 min ago" on hover; `aria-live` only on changes to Offline and back |
| Reconnect | Backoff 1, 2, 5, 10, 30 s with 20 % jitter; immediate retry on `online` and on tab becoming visible. After reconnect the first page is refetched and merged by id; caught-up captures count as new; banner "Reconnected, 12 captures caught up" auto-hides after 5 s |
| Deletes elsewhere | Removed rows fade out; a deleted selection becomes the "Deleted" placeholder |
| Background tab | Relative-time ticker (30 s) pauses while hidden; no animation runs while hidden; arrivals while hidden are not animated on return |
| Virtualization | Fixed row height (36/56 px), 10 rows overscan, selected row always mounted for focus and `aria-activedescendant`; pages of 100 loaded as the user scrolls; tested at 500 rows, works beyond |

## 7. Design tokens

Contrast target: WCAG 2.2 AA, 4.5:1 for all text including badges, 3:1 for borders that carry meaning
and focus rings. Ratios below were computed for the listed pairs.

| Role | Light | Dark | Checked pair (light / dark) |
|---|---|---|---|
| `bg` canvas | `#F6F5F1` | `#0E141B` | |
| `surface` panels | `#FFFFFF` | `#151D26` | |
| `surface-raised` sheets, menus | `#FFFFFF` | `#1C2632` | |
| `surface-sunken` code | `#EFEDE6` | `#0A0F15` | |
| `border` hairline | `#DCD8CE` | `#2A3644` | decorative |
| `border-strong` inputs | `#8C8576` | `#5A6B80` | 3.7 / 3.1 on surface |
| `text` | `#1B2330` | `#E6EAF0` | 14.5 on bg / 15.3 on bg |
| `text-muted` | `#4F5A6B` | `#A3AEBD` | 6.0 on sunken / 6.8 on raised |
| `accent` fill | `#D99A2B` | `#E0A23A` | `text-on-accent` 6.5 / 8.3 |
| `text-on-accent` | `#1B2330` | `#0E141B` | |
| `accent-text` links, active | `#8A5300` | `#F0B85A` | 5.6 on accent-soft / 9.5 on surface |
| `accent-soft` selection, arrival | `#FBEFD9` | `#3A2A10` | text 13.9 / 11.5 |
| `focus` ring 2 px, offset 2 px | `#2A5BD7` | `#7FA6FF` | 5.4 / 7.8 on bg |
| `success` | `#1E7A46` | `#4CC38A` | 5.4 / 7.7 on surface |
| `warning` | `#8A5A00` | `#E6B450` | 5.9 / 8.9 |
| `danger` | `#B42318` | `#FF8A7A` | 6.6 / 7.4 |
| `info` | `#2A5BD7` | `#7FA6FF` | 5.9 / 7.1 |

Method badges: tinted background with coloured text and the method name always spelled out (colour is
never the only carrier). Fixed width 64 px, mono 13 px 600.

| Method | Light text / bg | Dark text / bg | Contrast L / D |
|---|---|---|---|
| GET | `#1D5FB8` / `#E3EDFB` | `#8DB8FF` / `#14263F` | 5.3 / 7.6 |
| POST | `#186A3C` / `#E1F3E8` | `#5FD394` / `#11301F` | 5.8 / 7.7 |
| PUT | `#8A5300` / `#FBEFD9` | `#F0B85A` / `#3A2A10` | 5.6 / 7.7 |
| PATCH | `#6E40B8` / `#EFE7FB` | `#C3A3FF` / `#2A1E40` | 5.7 / 7.4 |
| DELETE | `#B42318` / `#FCE6E3` | `#FF8A7A` / `#3D1714` | 5.5 / 6.9 |
| OPTIONS | `#0F6E73` / `#DDF2F2` | `#5FD0D6` / `#0F2E30` | 5.2 / 7.9 |
| HEAD | `#3E5468` / `#E6EBF0` | `#A9BCCF` / `#1F2B37` | 6.6 / 7.4 |
| other | `#4F5A6B` / `#ECEAE4` | `#A3AEBD` / `#232D39` | 5.8 / 6.2 |

Status colours: 2xx `success`, 3xx `info`, 4xx `warning`, 5xx `danger`. Forward result: ok `success`,
failed `danger`, refused by SSRF guard `warning`, pending `text-muted` with spinner. Connection: as in
section 6.

| Type token | Size / line | Font | Use |
|---|---|---|---|
| `text-meta` | 13 / 18 | Sans 400 | Secondary: timestamps, sizes, hints, badges, table headers |
| `text-data` | 14 / 20 | Sans 400 | Default UI text, labels, buttons |
| `mono-data` | 14 / 20 | Mono 400 | Paths, URLs, headers, bodies, ids |
| `text-section` | 16 / 24 | Sans 600 | Section titles |
| `text-title` | 20 / 28 | Sans 600 | Page titles, capture URL in header |
| `text-hero` | 28 / 36 | Sans 600 | Home headline, empty-state URL |

Nothing renders below 13 px, ever; data is never below 14 px. Fonts: IBM Plex Sans and IBM Plex Mono,
self-hosted through `@fontsource` (no third-party font CDN), `font-display: swap`, fallback
`ui-monospace, SFMono-Regular, Menlo, monospace`. Rejected: JetBrains Mono, which reads well but breaks
the only cheap tie to the GlobalArt family.

| Scale | Values |
|---|---|
| Spacing | 2, 4, 8, 12, 16, 24, 32, 48 px (`space-0`..`space-7`) |
| Radii | 4 badges, inputs; 6 buttons, chips; 10 sheets, dialogs, toasts; 999 pills |
| Elevation | 0 flat (panels separated by `border`); 1 menus, popovers `0 4px 12px rgb(14 20 27 / 0.12)`; 2 sheets `0 8px 24px rgb(14 20 27 / 0.16)`; 3 dialogs, toasts `0 16px 40px rgb(14 20 27 / 0.22)`; dark adds a 1 px `border` on every elevated surface and uses `rgb(0 0 0 / 0.5)` |
| Motion | 120 ms hover, press; 180 ms menus, tabs, pill; 240 ms sheets and drawers (`cubic-bezier(0.2, 0, 0, 1)`); 1600 ms arrival fade (linear) |
| Reduced motion | Under `prefers-reduced-motion: reduce`: no translate, scale, or pulse; sheets appear in place; arrival fade replaced by the static unread dot; smooth scrolling off |

## 8. Components

| Component | States | Props (high level) |
|---|---|---|
| `AppShell` | wide, medium, narrow; rail collapsed | `rail`, `header`, `children` |
| `EndpointRail` | list, empty, loading | `endpoints`, `activeId`, `onCreate`, `onRemove`, `onRename` |
| `EndpointHeader` | normal, near-capacity (>= 90 %), full | `captureUrl`, `count`, `capacity`, `expiresAt`, `connection`, `follow` |
| `CopyButton` | idle, copied (1.5 s), failed | `value`, `label` |
| `ConnectionIndicator` | live, connecting, offline | `state`, `lastEventAt` |
| `FollowToggle` | on, off, paused | `value`, `onChange` |
| `FilterBar` | empty, active(n), popover/sheet variants | `filters`, `onChange`, `hiddenNewCount` |
| `CaptureList` | loading, empty, filtered-empty, populated, error | `items`, `selectedId`, `markedId`, `order`, `onSelect`, `onLoadMore` |
| `CaptureRow` | default, hover, selected, unread, arriving, marked-for-diff, deleting | `summary`, `variant` (one-line, two-line) |
| `NewArrivalsPill` | hidden, visible(n) | `count`, `onClick` |
| `Pager` | first/last disabled at ends | `position`, `total`, `onFirst`, `onPrev`, `onNext`, `onLast` |
| `MethodBadge` | per method | `method` |
| `StatusChip` | 2xx-5xx | `code`, `kind` (override, answered, forward) |
| `CaptureDetail` | loading, ready, large-pending, deleted, error | `capture`, `sections` |
| `SectionNav` | sticky, tabs variant | `sections`, `activeId` |
| `KeyValueTable` | collapsed, expanded, empty | `rows`, `collapseAfter`, `copyable` |
| `BodyViewer` | pretty, raw, invalid, large-truncated, worker-formatting, binary, image | `body`, `contentType`, `size`, `mode`, `wrap` |
| `HexView` | default | `bytes`, `limit` |
| `ForwardResultPanel` | pending, ok, failed, refused | `results`, `onResend` |
| `ResendDialog` | editing, sending, done, error | `rules`, `capture`, `onSend` |
| `CopyAsMenu` | closed, open | `capture`, `onCopied` |
| `DiffView` | loading, side-by-side, unified, identical | `a`, `b`, `hideVolatile` |
| `SettingsSheet` (now `EndpointWorkspace` sections, section 24) | view, dirty, saving, error | `endpoint`, `serverLimits`, `mode` |
| `ForwardingSection` / `ForwardRuleEditor` | list, empty, editing, field-error | `rules`, `onSave`, `onTest` |
| `EmptyEndpoint` | waiting | `captureUrl`, `responseSummary` |
| `StatePage` | gone, not-shared, error, not-found, sign-in | `kind`, `detail`, `actions` |
| `OfflineBanner` / `ReconnectBanner` | visible, hidden | `state`, `caughtUp` |
| `Toast` | info, success, danger, with action (Undo) | `message`, `action`, `duration` |
| `ConfirmDialog` | idle, busy | `title`, `body`, `confirmLabel`, `tone` |
| `ShortcutsDialog` | open | `shortcuts` |
| `LiveAnnouncer` | silent | none (subscribes to the live store) |

## 9. Screen data requirements

| Screen | Needs |
|---|---|
| List row | Capture id, method, path with query, size, content type, sender IP, received time, override status if any, latest forward outcome |
| Live stream | Events: capture created (row summary; flag when large per V5), capture deleted, all captures cleared, forward result for a capture, endpoint settings changed, endpoint expired |
| Catch-up | List captures with `seq` above the highest held (`after_seq`, ascending), all pages |
| Filters | Server-side filter by method set, override status class, time range, free text over headers and body; total count for the filtered set |
| Detail | Everything in C7, answered status, forward results (target, status, duration, error, time); raw body fetched separately |
| Header and settings | Capture URL, capture count, capacity, ring mode, expiry time and idle window, response config, CORS, delay |
| Server capabilities (`Instance`) | `auth_mode`, `limits.max_captures`, `limits.max_body_bytes`, `features.ring_mode_allowed`, `features.private_forward_targets_allowed`, `auth.provider_name` (null: neutral sign-in label), `user` |
| sso | Current user (name, email), endpoints the user created or that are shared with them, share list per endpoint |
| Forwarding | Rule CRUD, manual resend result or a pending marker later resolved by a live event |

## 10. Frontend engineering

| Concern | Choice | Reason | Rejected |
|---|---|---|---|
| Build | React 19, TypeScript strict, Vite | Given stack | |
| Routing | TanStack Router | Typed path and search params; filters live in the URL | React Router: untyped search params |
| Server state | TanStack Query | Cache that live events write into (`setQueryData`), infinite pages, retries | SWR: weaker cache mutation for streams |
| Live stream | Own `LiveSource` adapter over `EventSource` (swap to WebSocket if the backend chooses it) with backoff | Tiny, no dependency, interface fixed here | socket.io: the old single-replica design (spec section 12) |
| UI state | Zustand (selection, follow, unread, marked, prefs) | Small, selector subscriptions avoid list re-renders | Redux Toolkit: ceremony for a dozen fields |
| Virtualization | TanStack Virtual | Fixed rows, scroll anchoring control | react-window: less control over anchoring |
| Code viewer, diff | CodeMirror 6 read-only plus `@codemirror/merge`, lazy chunk | Viewport rendering survives 10 MiB, folding, search | Prism/highlight.js: renders the whole DOM |
| Primitives | Radix UI (Dialog, DropdownMenu, Popover, Tooltip, Toggle) | Accessible focus trapping and menus | Hand-rolled a11y |
| Styling | CSS Modules plus `tokens.css` custom properties | Small component set, themes by attribute | Tailwind: a build plugin for a few dozen components |
| Icons | `lucide-react` | Family tie with phoenix-ui | |
| i18n | i18next + react-i18next, ICU-free CLDR plural suffixes | Plurals for Russian, lazy bundles | Own `t()`: plural rules again |

State rule: server data only in the Query cache; Zustand holds ids and UI flags, never copies of captures.
Live events go through one reducer (`applyLiveEvent`) that merges by id, so catch-up and stream duplicates
are harmless.

```
web/
  index.html
  src/
    app/          router.tsx, providers.tsx, shell/
    routes/       home/, endpoint/, capture/, diff/, settings/, forwarding/, endpoints/, signin/, states/
    features/     live/, list/, detail/, body/, export/, filters/, forwarding/, shortcuts/, unread/
    components/   one folder per component in section 8, with its .module.css
    api/          client.ts, queries.ts, types.ts
    i18n/         setup.ts, keys.ts
    prefs/        store.ts, migrate.ts
    styles/       tokens.css, base.css, fonts.css
    test/         msw handlers, fixtures
  e2e/            Playwright specs
```

| Key | Action | Scope |
|---|---|---|
| `j` / `Down` | Next capture (older in newest-first order) | Endpoint view |
| `k` / `Up` | Previous capture | Endpoint view |
| `Shift+K` / `Home` | First (newest), turns follow on | Endpoint view |
| `Shift+J` / `End` | Last | Endpoint view |
| `f` | Toggle follow | Endpoint view |
| `/` | Focus search | Endpoint view |
| `c` | Copy capture URL | Endpoint view |
| `y` | Copy selected as curl | Capture selected |
| `r` | Pretty / raw | Capture selected |
| `w` | Wrap body | Capture selected |
| `d` | Mark for diff, or diff with marked | Capture selected |
| `Delete` | Delete selected (5 s Undo toast, server call after) | Capture selected |
| `s` / `Shift+F` | Open settings / forwarding | Endpoint view |
| `n` | New endpoint | Anywhere |
| `?` | Shortcuts dialog | Anywhere |
| `Esc` | Close sheet, dialog, or narrow detail | Anywhere |

Shortcuts are ignored while focus is in an input or the code viewer. Single-key shortcuts can be turned
off in preferences (WCAG 2.1.4).

Accessibility:

| Area | Rule |
|---|---|
| List | `role="listbox"` with `aria-activedescendant`, options carry `aria-setsize` and `aria-posinset`; the selected option is always mounted |
| Announcements | One visually hidden `role="status"` (`aria-live="polite"`) region, at most one message per 5 s, aggregated: "3 new captures, latest POST /stripe"; the list itself is not live; preference to mute |
| Focus | Arrivals never move focus; narrow detail moves focus to its heading and returns it to the row on back; sheets and dialogs trap focus and return it to the trigger; skip link to the list |
| Visuals | AA contrast (section 7), 2 px focus ring, targets >= 24 px (44 px for primary actions on narrow), colour never the only carrier |
| Motion | Reduced-motion rule in section 7 |

Internationalisation:

| Rule | Detail |
|---|---|
| Source of truth | OwnLate project `webhooks-live`, source `en_US`, target `ru` at launch; no translation files committed |
| Delivery | `pnpm i18n:pull` fetches the bundle through the public OTA key at build time; the bundle is embedded in the build; a self-hosted instance never calls OwnLate at runtime |
| Key shape | `wl.<area>.<element>[.<qualifier>]`: fixed product prefix `wl` (the slug has a hyphen, which reads badly inside dotted keys), lowercase areas, camelCase elements: `wl.endpoint.header.copyUrl`, `wl.capture.section.headers`, `wl.live.status.reconnecting`, `wl.state.gone.title` |
| Plurals | i18next suffixes: `wl.live.announce.newCaptures_one`, `_few`, `_many`, `_other` |
| Placeholders | Named, `{{count}}`, `{{method}}`; no string concatenation |
| Guard | CI fails when a key used in code is missing from the pulled bundle; keys are referenced as literals so the check can grep them |
| Formats | Dates, sizes, relative times through `Intl` in the active locale; captured data (headers, bodies) is never translated |

Local preferences: `localStorage` key `webhooks-live:prefs:v1` (theme, follow default, order, list width, rail
collapsed, pretty default, wrap, section collapse, toast, announcements, single-key shortcuts, volatile
header list, language); `webhooks-live:endpoints:v1` (up to 50 entries: id, label, added, last opened, last seen
capture id, used for "new since last visit"). Versioned with a migration function; unreadable data resets
to defaults. Theme is applied before first paint by a hashed inline script in `index.html`.

Untrusted content rules: no `dangerouslySetInnerHTML`; captured HTML and SVG are text, except the HTML part
of an email capture, which renders only inside the sandboxed frame of section 18.2; header values are
not auto-linked; the raw body opens only through the backend's safe raw route (C12); image preview uses a
blob URL with a fixed type allow-list.

| Test layer | Covers |
|---|---|
| Vitest (pure) | curl quoting (single quote, newline, binary), HAR shape, JSON export, body sniffing, JSON/XML pretty, diff normalisation, `applyLiveEvent` merge and dedup, unread counting, backoff schedule, prefs migration, key-shape check |
| Vitest + Testing Library + MSW | Row and badge rendering, detail sections per content type, follow pause rule, filter-hidden counter |
| Playwright | Create endpoint, send a request with the `request` fixture and see it live without reload; follow mode; reconnect catch-up (stream route aborted then restored); 410 page; keyboard navigation; narrow 360 px flows; axe checks on main screens in both themes; screenshots at 360, 1024, 1440 |

| Budget | Target |
|---|---|
| Initial JS for endpoint view, gzip | <= 150 KB (CodeMirror excluded) |
| CodeMirror chunk, gzip | <= 180 KB, loaded on first body view |
| First render of endpoint view on a fast local network | LCP <= 1.5 s |
| Event received to row painted | <= 100 ms |
| 500-row list | 60 fps scroll, no long task > 50 ms on arrival |
| Memory | Summaries for loaded pages plus an LRU of 20 full captures |
| Hidden tab | No timer more frequent than once per minute |

## 11. Milestones

| # | Scope | Done when |
|---|---|---|
| F0 | Vite + React + TS scaffold, lint, format, typecheck, Vitest, Playwright smoke, `tokens.css`, fonts, theme switch without flash | CI runs all checks green; light/dark screenshot of a token sample page |
| F1 | API client, query hooks, MSW handlers from section 9 against the API document | Hooks tested against MSW for list, detail, create, delete |
| F2 | i18n setup, `i18n:pull`, missing-key guard | A missing key fails CI; English and Russian render on the sample page |
| F3 | App shell, routes, `StatePage` (gone, not-shared, error, not-found, sign-in), offline banner | Playwright visits every route and state with mocked answers |
| F4 | Home / create, custom response, my endpoints storage and rail | Create opens the empty state; list survives reload; labels editable |
| F5 | Endpoint view: header, virtualized list, selection by URL, pager, delete with Undo, empty state, all three layouts | 500 mocked rows scroll smoothly; deep link selects the capture; 360 px layout matches section 3 |
| F6 | Live: `LiveSource`, `applyLiveEvent`, arrival fade, scroll anchoring, new pill, follow, title and favicon, connection indicator, reconnect catch-up, announcer | Playwright: request appears live; selection and scroll do not move; reconnect catches up without duplicates |
| F7 | Capture detail: sections, key-value tables, form and multipart, body viewer (lazy CodeMirror, worker pretty-print, large, binary, image) | Fixtures for JSON, invalid JSON, XML, text, 5 MiB, binary, PNG render as specified |
| F8 | Copy-as and downloads: curl, HAR, JSON, raw | Unit tests for quoting pass; exported curl reproduces the capture in Playwright |
| F9 | Filters and search in URL, hidden-by-filter counter, text-query refetch | Filtered link reopens the same view; counter counts non-matching arrivals |
| F10 | Workspace sections General and Response (literal response) on F16: response config, CORS, delay, ring mode, delete all, delete endpoint; sso sharing moves to F28 | Saved settings change what a test request receives |
| F11 | Forwarding workspace section on F16, rule editor, forward result panel, resend dialog | Forward result appears on a capture live; SSRF refusal shows on the field |
| F12 | Diff view, mark and compare, hide volatile | Two fixtures show header and JSON body differences side-by-side and unified |
| F13 | Shortcuts, shortcuts dialog, a11y pass | Every shortcut in the table has a Playwright case; axe reports no serious issues |
| F14 | Hardening: screenshot baselines at 360/1024/1440 in both themes, bundle-size check, 500-row arrival perf check | Budgets in section 10 enforced in CI |

F6 (asynchronous stream handling) and F7 (rendering untrusted bodies) fall in the risky areas of the
working rules and should go to the stronger coder.

## 12. Open questions for the owner

1. IBM Plex is licensed OFL-1.1, which the dependency policy in [licensing.md](licensing.md) does not
   list. Allow OFL for font assets, or use the system font stack and give up the family tie?
2. Is per-user sharing in sso mode (spec section 8) part of the first release? If not, F10 drops
   "Shared with" and the not-shared state (403 `forbidden` on a disallowed action) stays as is.
3. Launch languages: English and Russian only, or more targets in the `webhooks-live` OwnLate project?

## 13. Parity scope

Sections 13 to 31 extend this design to full functional parity with commercial request catchers (owner
decision, 2026-11-21). The backend side is designed in parallel (`parity.md` and additions to the product,
architecture and API documents); this document names what the interface needs by capability, never by
backend milestone. Where a field or route name is needed before the API document fixes it, the name here is
a placeholder and the API document wins.

| # | Capability | Interface surface | Section |
|---|---|---|---|
| 1 | Response templating | Workspace section Response: template editor, variable picker, live preview, errors | 16 |
| 2 | Rules | Workspace section Rules: rule list, builder, script step, test runner, run log; capture section Rules | 17 |
| 3 | Email capture | Addresses popover, `MAIL` rows, email detail with sandboxed HTML | 18 |
| 4 | DNS capture | Addresses popover, `DNS` rows, DNS detail | 18 |
| 5 | Endpoint management | Workspace sections General and Access; folders and labels in the rail; combined inbox; clone | 15, 19 |
| 6 | API tokens, API reference, CLI | `/account/tokens`, `/docs/api`, workspace section Local forwarding | 22 |
| 7 | Request composer | `/send`, replay from a capture | 20 |
| 8 | Scheduled requests | Workspace section Schedules | 20 |
| 9 | Export and import | Export dialog with server job progress, import dialog | 21 |
| 10 | Notifications | Account channels, workspace section Notifications, Notify step | 22 |
| 11 | Quotas and usage | `/account/usage`, meters in General and the header | 22 |
| 12 | Search and filters | Filter builder, extended URL filter, saved filters | 23 |
| 13 | Account area | Account menu, `/account`, merged `/endpoints`, `/settings` preferences | 22 |

| Principle | Rule |
|---|---|
| One engine | Templates, rules, scripts, dry runs and search are evaluated only on the server. The interface never re-implements their semantics, so a preview cannot drift from what senders get |
| Catalogue-driven | Variables, condition subjects and operators, step types with their fields, channel kinds and token scopes come from server catalogues and are drawn by generic widgets; a new step type needs no interface release |
| Syntax-agnostic | The interface does not hard-code template or script syntax: the variable picker inserts the catalogue's insertion text, highlighting uses delimiters from the catalogue. Template syntax in the wireframes is illustrative |
| One list, three kinds | HTTP, email and DNS captures share list, selection, live updates, filters, export and diff; only the row text and the detail differ |
| Hidden, not disabled | An area whose capability the server does not offer (Instance flags) is not shown at all; a role that cannot act sees the control read-only with the existing "Your role does not allow this" reason |
| Own vocabulary | Rules, Steps, Conditions, Test run, Run log, Addresses, Alias, Inbox, Folder, Label, Composer, Schedule, Channel, Access, Configure. No product term, text or layout of any commercial service ([licensing.md](licensing.md), rule 3) |

## 14. Information architecture additions and routes

| Place | Addition |
|---|---|
| Rail | `Inbox` entry with the unread total on top; endpoints grouped by folder (one level, collapsible, `Unfiled` last); a label filter chip above the list; `Account` button at the bottom (signed in: initials and menu; anonymous where sign-in exists: `Sign in`) |
| Endpoint header, line 1 | `[Addresses n]` popover after `[Copy curl]` (HTTP URL, aliases, email address, DNS name) |
| Endpoint header, line 2 | `[Rules n] [Configure] [...]` replace `[Settings] [Forwarding]`; `[...]` holds Compose request, Export, Import, Clone, Local forwarding, Schedules, Notifications, Shortcuts |
| List toolbar | Kind filter; `Select` mode (`x`) with bulk Export and Delete |
| Capture detail | Actions `Replay in composer` (`Shift+R`), `Test rules with this capture`, `Preview response with this capture`; sections `Response sent` and `Rules` (section 17.6) |
| Account menu | Profile, My endpoints, API tokens, Notifications, Usage and limits, Preferences, API reference, Sign out |

Route additions (section 2 keeps its rows; the two configuration rows and the filter row there now point here):

| Route | Screen | Notes |
|---|---|---|
| `/e/:endpointId/settings` | Endpoint workspace, General | Section 15 |
| `/e/:endpointId/settings/response` | Response template editor | `?preview=<captureId>` pins the preview capture |
| `/e/:endpointId/settings/rules` | Rule list, nothing open | `?test=all&capture=<id>` opens Test all rules |
| `/e/:endpointId/settings/rules/new` | New rule | `?from=blank\|script\|<ruleId>` (duplicate); routed as `rules/:ruleId` with `new` |
| `/e/:endpointId/settings/rules/:ruleId` | Rule editor with tester | `?tab=build\|test\|runs&capture=<id>&run=<runId>`; `step=<n>` is planned |
| `/e/:endpointId/settings/forwarding` | Forwarding | `/e/:endpointId/forwarding` redirects here |
| `/e/:endpointId/settings/access` | Access | Password, sender allow list, members |
| `/e/:endpointId/settings/notifications` | Notification channels of the endpoint, browser push, e-mail addresses | Shipped form of section 22.5 |
| `/e/:endpointId/settings/schedules` | Schedule list | |
| `/e/:endpointId/settings/schedules/:scheduleId` | Schedule editor and history | Planned; the editor opens inside the section without its own route |
| `/e/:endpointId/settings/cli` | Local forwarding instructions | |
| `/e/:endpointId/c/:captureId/html` | Email HTML full view | Planned with F26 (needs the backend HTML route) |
| `/e/:endpointId/unlock` | Password unlock | `?next=` returns to the original route |
| `/inbox` | Combined inbox | `?scope=all\|folder:<id>\|label:<name>` plus filter parameters |
| `/inbox/:endpointId/:captureId` | Inbox, capture selected | Narrow: detail full screen |
| `/send` | Composer | `?endpoint=<id>&capture=<id>` prefills a replay; the API reference hands a request over in memory (section 22.3) |
| `/endpoints` | My endpoints, merged | `?folder=&label=&source=local\|account\|shared`; `?import=1` opens import |
| `/account` | Profile and sign out | Signed in only |
| `/settings/tokens` | API tokens | |
| `/usage` | Usage and limits | |
| `/email/verify`, `/notifications/disable` | Landing pages of the verification and unsubscribe links | Outside the shell |
| `/account/notifications` | Account-wide channel list | Planned; channels are per endpoint in this release |
| `/settings` | Preferences | Already routed; anchors `#appearance`, `#live`, `#editors`, `#shortcuts`, `#data` |
| `/docs/api`, `/docs/api/:operationId` | API reference | |
| `?export=selected\|filtered\|all` | Export dialog over endpoint or inbox routes | Closing removes the parameter |

| Change to section 2 | Why |
|---|---|
| `/e/:endpointId/settings` is a page (workspace), not a sheet | Section 15 |
| `/e/:endpointId/forwarding` becomes a redirect | Forwarding is a workspace section |
| The filter row is replaced by the parameter table in section 23 | Body, header, kind, sender, rule and absolute time filters |

The backend serves `index.html` for the routed paths only (`whl_web::UI_ROUTES`, architecture section 10):
`/inbox`, `/send`, `/account`, `/usage`, `/settings/tokens`, `/docs/api`, the link landing pages and the
`/e/:endpointId/...` routes above. A new UI route needs a matching `UI_ROUTES` entry. All new root
segments are shorter than 24 characters, so they cannot be read as endpoint identifiers.

## 15. Endpoint workspace

**Decision: endpoint configuration moves from the 480 px side sheet to a workspace route with a section
nav.** The rule builder with its tester and the template editor with its preview each need 900 px or more;
ten sections do not fit one sheet; each section gets a deep link. Rejected: a wider sheet (still covers the
list, and nested sheets for the rule editor) and a modal per section (no deep links, no side-by-side test).
Cost: configuration is no longer drawn over the live list; the workspace header keeps the capture URL, the
connection indicator and a `[< Captures n new]` link, and editors pick captures through `CapturePicker`.

| Section | Contents | Edit role | Nav badge |
|---|---|---|---|
| General | Title, folder, labels, addresses and aliases, lifetime, overflow mode, usage meters, clone, export, import, danger zone | Editor; delete endpoint owner | - |
| Response | Template editor (section 16), delay, CORS | Editor | - |
| Rules | Rule list, editor, tester, runs (section 17) | Editor; viewers see read-only rules and runs | Count; danger dot when a rule failed in the last 24 h |
| Forwarding | F11 content unchanged | Editor | Count |
| Access | Viewer password (not in `sso`), sender allow list, members and invitations (sso or accounts) | Owner | - |
| Notifications | Triggers for this endpoint (section 22.5) | Editor | - |
| Schedules | Scheduled requests (section 20.2) | Editor | Count |
| Local forwarding | CLI instructions (section 22.4) | Viewer | - |

| Element | Wide | Medium | Narrow |
|---|---|---|---|
| Rail | Stays; auto-collapses to 56 px on Response and rule editor routes, restored on leaving | Drawer | Drawer |
| Section nav | Left column 200 px | Horizontal tab strip under the header, scrolls, `[>]` overflow | Section list page, each section full screen with back |
| Form width | Forms max 720 px; Response and Rules use the full width | Full width | Full width |
| Save | Sticky save bar at the bottom of the section | Same | `[Save]` in the top bar while dirty |
| Back to captures | `[< Captures n new]`, `Esc` when no field is focused | Same | `[<]` in the top bar |

Wide (1440 px), General:

```
+---------------+---------------------------------------------------------------------------------------------+
| webhooks.live | [< Captures  2 new]  stripe-dev   https://hooks.example/k7Qm2xTa9vNc  [Copy] [Addresses 3]  |
|               |                                                    (*) Live   [Rules 3] [Configure] [...]   |
+---------------+--------------------+------------------------------------------------------------------------+
| INBOX     14  | CONFIGURE          | GENERAL                                                                |
| FOLDERS       | > General          | Title   [stripe-dev                                  ]                 |
| v Payments    |   Response         | Folder  [Payments          v]   Labels [stripe x] [test x] [+ Label]   |
|   stripe-dev  |   Rules          3 |                                                                        |
|   paypal-sb   |   Forwarding     1 | ADDRESSES                                                              |
| v Unfiled     |   Access           | HTTP    https://hooks.example/k7Qm2xTa9vNc                      [Copy] |
|   github-app  |   Notifications    | Alias   https://hooks.example/a/stripe-dev             [Copy] [Remove] |
|               |   Schedules      2 |         [+ Add alias]                                                  |
|               |   Local forwarding | Email   k7qm2xta9vnc@in.hooks.example                           [Copy] |
|               |                    | DNS     k7qm2xta9vnc.dns.hooks.example                          [Copy] |
|               |                    |                                                                        |
|               |                    | LIFETIME                                                               |
|               |                    | (o) Delete after [7 days  v] without activity      next: 28 Nov 14:02  |
|               |                    | ( ) Delete on    [2026-12-31   ]                                       |
|               |                    | ( ) Keep until deleted                                                 |
|               |                    | When full   (o) Refuse new captures (410)   ( ) Drop the oldest        |
|               |                    |                                                                        |
|               |                    | USAGE   captures 312 / 500 [######----]   storage 18.2 MiB             |
|               |                    |         rule runs today 1 204 / 10 000              [About limits]     |
|               |                    |                                                                        |
|               |                    | [Clone endpoint]     [Export...]     [Import...]                       |
|               |                    | DANGER ZONE   [Delete all captures]   [Delete endpoint]                |
| [+ New]       |                    |                                    Unsaved changes  [Discard] [Save]   |
| [Account v]   |                    |                                                                        |
+---------------+--------------------+------------------------------------------------------------------------+
```

Medium (1024 px), General:

```
+----------------------------------------------------------------------------------------------+
| [=] webhooks.live  [< Captures 2 new]  https://hooks.example/k7Qm2xTa9vNc [Copy]   (*) Live  |
+----------------------------------------------------------------------------------------------+
| >General  Response  Rules 3  Forwarding 1  Access  Notifications  Schedules 2  Local fw  [>] |
+----------------------------------------------------------------------------------------------+
| GENERAL                                                                                      |
| Title   [stripe-dev                          ]     Folder [Payments          v]              |
| Labels  [stripe x] [test x] [+ Label]                                                        |
| ADDRESSES                                                                                    |
| HTTP    https://hooks.example/k7Qm2xTa9vNc                                        [Copy]     |
| Alias   https://hooks.example/a/stripe-dev                               [Copy] [Remove]     |
| Email   k7qm2xta9vnc@in.hooks.example                                             [Copy]     |
| DNS     k7qm2xta9vnc.dns.hooks.example                                            [Copy]     |
| LIFETIME  (o) Delete after [7 days v] idle   ( ) Delete on [date]   ( ) Keep until deleted   |
| When full (o) Refuse new captures   ( ) Drop the oldest                                      |
| USAGE     captures 312 / 500 [######----]   storage 18.2 MiB   [About limits]                |
| [Clone endpoint]  [Export...]  [Import...]      [Delete all captures]  [Delete endpoint]     |
|                                                          Unsaved changes  [Discard] [Save]   |
+----------------------------------------------------------------------------------------------+
```

Narrow (360 px), section list and General:

```
+------------------------------------+    +------------------------------------+
| [<] Configure . stripe-dev         |    | [<] General                [Save]  |
+------------------------------------+    +------------------------------------+
| General                          > |    | Title                              |
| Response                         > |    | [stripe-dev                    ]   |
| Rules                          3 > |    | Folder [Payments              v]   |
| Forwarding                     1 > |    | Labels [stripe x] [test x] [+]     |
| Access                           > |    | ADDRESSES                          |
| Notifications                    > |    | HTTP   k7Qm2x...vNc         [Copy] |
| Schedules                      2 > |    | Alias  a/stripe-dev         [Copy] |
| Local forwarding                 > |    | Email  k7qm2x...@in.hoo..   [Copy] |
+------------------------------------+    | DNS    k7qm2x...dns.hoo..   [Copy] |
                                          | LIFETIME                           |
                                          | (o) After [7 days v] idle          |
                                          | ( ) On [date]     ( ) Keep         |
                                          | When full [Refuse new       v]     |
                                          | USAGE  312 / 500 [######----]      |
                                          | [Clone] [Export] [Import]          |
                                          | [Delete all]  [Delete endpoint]    |
                                          +------------------------------------+
```

Wide, Access (medium uses the same content under the tab strip):

```
+--------------------+----------------------------------------------------------------------------------+
| CONFIGURE          | ACCESS                                                                           |
|   General          | VIEWING                                                                          |
|   Response         | [x] Require a password to open this endpoint in the interface and API            |
|   Rules          3 |     Password [**************      ] [Show]  [Change] [Remove]      set 2 d ago   |
|   Forwarding     1 |     Senders are not affected: the capture URL, email and DNS stay open.          |
| > Access           |                                                                                  |
|   Notifications    | SENDERS                                                                          |
|   Schedules      2 | [x] Accept HTTP captures only from these addresses                               |
|   Local forwarding |     [203.0.113.0/24 x] [2001:db8::/32 x] [+ Address or range        ]            |
|                    |     Others get 403 and are (o) not stored  ( ) stored and marked refused         |
|                    |     Your address 198.51.100.4 is outside the list.            [Add it]           |
|                    |                                                                                  |
|                    | MEMBERS                                                            [Invite]      |
|                    | Alex (you)         alex@example.com        Owner                                 |
|                    | Dev Ops            ops@example.com         [Editor v]               [Remove]     |
|                    | sam@example.com    invited 1 h ago         [Viewer v]      [Resend] [Revoke]     |
|                    | Editors change settings and rules and delete captures. Viewers only read.        |
|                    |                                          Unsaved changes  [Discard] [Save]       |
+--------------------+----------------------------------------------------------------------------------+
```

Narrow, Access:

```
+------------------------------------+
| [<] Access                 [Save]  |
+------------------------------------+
| VIEWING                            |
| [x] Password to open               |
| [**************    ] [Show]        |
| Senders are not affected.          |
| SENDERS                            |
| [x] Only from these addresses      |
| [203.0.113.0/24 x]                 |
| [2001:db8::/32 x]                  |
| [+ Address or range        ]       |
| Others: 403 and                    |
| (o) not stored ( ) marked refused  |
| Your 198.51.100.4 is outside.      |
|                          [Add it]  |
| MEMBERS                  [Invite]  |
| Dev Ops . ops@example.com          |
| [Editor v]               [Remove]  |
| sam@example.com . invited          |
| [Viewer v]       [Resend][Revoke]  |
+------------------------------------+
```

| Behaviour | Rule |
|---|---|
| Save model | Forms in General, Response, Access, Notifications have a draft and one save bar (`Mod+S`). Switches that are a single state change (rule on/off, schedule pause, member role) apply at once with an Undo toast. Rules and schedules save one item at a time |
| Dirty guard | Leaving a section with unsaved changes opens a `ConfirmDialog` (Keep editing, Discard); closing the tab uses `beforeunload` only while dirty |
| Conflicts | Every save sends the version the draft started from; a version mismatch, or an `endpoint_updated` / rule change event while dirty, shows `ConflictBanner`: "Changed elsewhere. Reload, or keep yours and overwrite". No silent merge. Shipped: the server has no version check yet, so the client re-reads before saving (endpoints compare fields, rules compare the list `version`), which leaves a short race; rules switch to `If-Match` when api.md section 15 "Edit conflicts" ships |
| Lifetime | Options shown are those the instance allows: sliding (inactivity window up to the instance maximum), fixed date, keep until deleted. Each option states the outcome: "The endpoint and its captures are deleted; senders then get 410" |
| Aliases | Free text, availability checked while typing (debounced 400 ms); each alias row shows its full URL with Copy |
| Clone | Dialog: new title (default "<title> copy"), checkboxes Response, Rules, Forwarding, Notifications, Schedules (paused), Labels and folder, Access list; members, password and captures are never copied. Opens the new endpoint's General section |
| Viewer password | Setting it requires typing it twice; the value is never displayed back after saving (only "set 2 d ago"). Endpoints with a password answer the interface with a locked state, rendered by `StatePage` kind `locked` at `/e/:endpointId/unlock`; the unlock lasts for the browser session |
| Sender allow list | Chips of IPv4/IPv6 addresses or CIDR ranges, validated on entry; the viewer's own address (from Instance) is shown with "outside the list" and `[Add it]` |
| Members | Owner sees invite, role change, remove, pending invitations with Resend and Revoke; others see the list read-only |

## 16. Response templating editor

**Decision: an explicit `Use variables` switch per endpoint response.** Off, the body and header values are
literal, as today; on, they are templates. Existing literal bodies that happen to contain template
delimiters therefore never change meaning. Rejected: always-on templating (silently breaks such bodies).

**Decision: the preview is rendered by the server against a chosen capture.** Rejected: a client-side
renderer (two engines drift; the server one is the one senders get).

Wide (rail collapsed), editor and preview side by side:

```
+-----+-------------------------------------------------------------------------------------------------------+
| (o) | [< Captures 2 new]  stripe-dev . Configure / Response                     (*) Live  [Save Ctrl+S]     |
+-----+---------------------------------------------------------+---------------------------------------------+
| [I] | RESPONSE                         Use variables [on]     | PREVIEW with [POST /stripe . 12s   v] [Pin] |
| [P] | Status [200  ]   Type [application/json         v]      | 200 . application/json . 212 B . 4 ms       |
| [S] | Delay  [0 s --o--------]   [ ] CORS                     | HEADERS                                     |
|     | HEADERS                                                 | x-order-id      ord_77                      |
|     | x-order-id [{{request.json.data.id}}    ] [{}] [x]      | x-mode          test                        |
|     | x-mode     [{{request.query.mode}}      ] [{}] [x]      | BODY                          [Pretty|Raw]  |
|     | [+ Header]                                              | 1  {                                        |
|     | BODY                            [{} Insert variable]    | 2    "received": "evt_1Q",                  |
|     | 1  {                                                    | 3    "amount": 2000,                        |
|     | 2    "received": "{{request.json.id}}",                 | 4    "at": ""                               |
|     | 3    "amount": {{request.json.data.amount}},            | 5  }                                        |
|     | 4 !  "at": "{{now | isodate}}"                          | ! Last valid render; 1 error in the         |
|     |           ~~~~~~~ Unknown filter "isodate"              |   editor. Unknown values render empty.      |
|     | 5  }                                                    | [Send a test request to this endpoint]      |
|     | ERRORS 1   4:19 Unknown filter "isodate"   [Go]         |                                             |
|     | 212 B of 64 KiB . Esc then Tab leaves the editor        |                                             |
+-----+---------------------------------------------------------+---------------------------------------------+
```

Variable picker (popover from `[{}]`, `[{} Insert variable]`, or typing the template opener):

```
+----------------------------------------------------------------+
| [/ Filter variables                    ]  sample: POST /stripe |
+----------------------------------------------------------------+
| REQUEST                                                        |
|   method                         "POST"                        |
|   path                           "/stripe"                     |
| > query.mode                     "test"                        |
|   header.stripe-signature        "t=1732...,v1=9a..."          |
|   json.id                        "evt_1Q"                      |
|   json.data.amount               2000                          |
| TIME                                                           |
|   now                            2026-11-21T14:02:11Z          |
| RULE VARIABLES                                                 |
|   vars.amount                    set by rule 1, step 1         |
| HELPERS                                                        |
|   uuid                           a new random UUID             |
+----------------------------------------------------------------+
| Enter insert . Right expand . Esc close                        |
+----------------------------------------------------------------+
```

Medium: preview is a resizable bottom panel (default 35 % height, `[Hide]` remembered):

```
+----------------------------------------------------------------------------------------------+
| [=] [< Captures 2 new]  stripe-dev . Configure / Response         (*) Live  [Save Ctrl+S]    |
+----------------------------------------------------------------------------------------------+
|  General >Response  Rules 3  Forwarding 1  Access  Notifications  Schedules 2  Local fw  [>] |
+----------------------------------------------------------------------------------------------+
| Status [200 ]  Type [application/json     v]  Delay [0 s --o----]  [ ] CORS   Variables [on] |
| HEADERS  x-order-id [{{request.json.data.id}}       ] [{}] [x]                    [+ Header] |
| BODY                                                                   [{} Insert variable]  |
| 1  {                                                                                         |
| 2    "received": "{{request.json.id}}",                                                      |
| 4 !  "at": "{{now | isodate}}"          4:19 Unknown filter "isodate"                        |
| 5  }                                                                                         |
| 212 B of 64 KiB . 1 error                                                                    |
+----------------------------------------------------------------------------------------------+
| PREVIEW with [POST /stripe . 12s v] [Pin]   200 . application/json . 212 B   [Resize] [Hide] |
| { "received": "evt_1Q", "amount": 2000, "at": "" }                                           |
+----------------------------------------------------------------------------------------------+
```

Narrow: `[Edit] [Preview] [Errors n]` segments:

```
+------------------------------------+    +------------------------------------+
| [<] Response               [Save]  |    | [<] Response               [Save]  |
+------------------------------------+    +------------------------------------+
| [Edit]  Preview   Errors 1         |    |  Edit  [Preview]  Errors 1         |
+------------------------------------+    +------------------------------------+
| Variables [on]                     |    | With [POST /stripe . 12s      v]   |
| Status [200 ]  [application/js v]  |    | 200 . application/json . 212 B     |
| Delay [0 s --o----]   [ ] CORS     |    | HEADERS                            |
| HEADERS                       [+]  |    | x-order-id    ord_77               |
| x-order-id                         |    | x-mode        test                 |
| [{{request.json.data.id}}  ] [{}]  |    | BODY                 [Pretty|Raw]  |
| BODY                    [{} Var]   |    | {                                  |
| 1  {                               |    |   "received": "evt_1Q",            |
| 2    "received": "{{request.j      |    |   "amount": 2000,                  |
| 4 !  "at": "{{now | isodate}}"     |    |   "at": ""                         |
| 5  }                               |    | }                                  |
| 212 B of 64 KiB . 1 error          |    +------------------------------------+
+------------------------------------+
```

| Topic | Rule |
|---|---|
| Editor | `CodeEditor` (section 24) with the body content type's language plus template delimiter highlighting (`accent-text` on `accent-soft`); header values are single-line `CodeEditor` instances so tokens highlight there too; status and content type stay plain fields |
| Autocomplete | Typing the template opener opens completion from the variable catalogue of the preview capture; each entry shows path, type and sample value |
| Variable picker | Groups from the catalogue (request, time, rule variables, helpers); JSON body keys of the preview capture appear as concrete paths; filter by substring; `Enter` inserts the catalogue insertion text at the cursor of the field that opened it |
| Preview capture | `CapturePicker`, default `Latest` which follows new arrivals; `[Pin]` fixes the current one and writes `?preview=`; empty endpoint: `[Send a test request to this endpoint]` posts a fixed sample to the capture URL (same origin) and pins the result |
| Preview trigger | Debounced 500 ms after the last edit, also `Mod+Enter`; in-flight renders are aborted; only the newest result is shown (request sequence number) |
| Errors | Server diagnostics (line, column, length, message) become lint underlines, gutter markers and an `ERRORS n` list under the editor with `[Go]`; the preview keeps the last valid render with a warning line; Save is disabled while errors exist |
| Warnings | Unknown variables render empty and appear as warnings, not errors; Save stays enabled |
| Size | Counter of the rendered-before-variables body against the instance limit (64 KiB today); over the limit is an error |
| Role | Viewers see the editor read-only and can still preview |

## 17. Rules editor

### 17.1 Model

| Part | Meaning | Interface |
|---|---|---|
| Rule | Named, ordered, on/off. Rules run top to bottom for every capture of a matching kind | Card in the rule list |
| Trigger | Capture kinds the rule applies to: HTTP, Email, DNS (any combination) | Checkboxes in `WHEN`. Not in v1 (below) |
| Conditions | `all` or `any` of predicates; a predicate is subject, operator, value; no nesting | `ConditionGroup` of `ConditionRow`s; an empty group always matches |
| Subject | Catalogue entry: method, path, query parameter, header, JSON path in body, body text, form field, sender address, email from, to, subject, DNS name, type, a rule variable | First select; parameter field (header name, JSON path) when the subject needs one |
| Step | One action, run in order when the conditions match | `StepCard` |
| Step guard | Optional single predicate "only if" on a step | Line on the card; the only branching there is. Not in v1 (below) |
| Variables | Request data plus `vars.*` produced by steps; later steps and the response template can use them | Shown as "Produces vars.x" on the step, and in the tester |
| Script step | A step whose body is a script in the server's sandboxed language | `ScriptEditor` inside the card; a rule made from the Script template is one full-height Script step |
| On match | `Next rule` (default) or `Stop here` (later rules are skipped) | Radio at the end of the rule. Not in v1 (below) |
| Phase | Steps that shape the response run before the sender is answered, the rest after; reported per step type by the catalogue | Badge `Before response` on such steps; ordering rule enforced by the server and shown as a field error |

**v1 scope (decided 2026-11-22).** Triggers, step guards and "On match" are not part of the rule model in v1: the
server refuses those fields (api.md section 15). The editor does not show them; the wireframes keep them as the
later design. What users can do today: a condition on `request.kind` instead of trigger checkboxes, a `Stop`
step at the end of a rule instead of "Stop here", a separate rule instead of a guarded step. Reason: each would add
a field to the stored model and the plan cut, while the condition tree and the `stop` step already express them;
adding them later is additive. The rule condition editor also allows the server's nesting (`all`, `any`, `not`)
rather than the flat group above. `http` steps can be placed only after the last Respond step; the server refuses
other positions with a field error on the step.

**Decision: an ordered list of cards, not a node graph.** Rule flows here are linear (extract, decide,
respond, send); one level of guards covers the branching that occurs; a list reads top to bottom,
works with a keyboard and a screen reader, fits 360 px, and diffs well. Rejected: a node-graph canvas (needs
pointer precision, a heavy library, and a parallel accessible representation anyway).

**Decision: a script is a step, not a rule mode.** Visual steps and scripts mix in one rule; "script mode"
is the rule template that holds a single Script step. Rejected: a visual/script toggle per rule (conversion
in either direction is lossy and invites a promise the server cannot keep).

**Decision: step forms are drawn from the step catalogue's field schemas.** Field kinds: text, template,
number, duration, boolean, select, key-value list, JSON path, header name, channel reference, script.
Rejected: hand-written form per step type (every new step needs a release) and generic JSON Schema form
libraries (weak accessibility control, large).

### 17.2 Wireframes

Wide (1440 px, rail collapsed): rule list, builder, tester.

```
+-----+--------------------------------------------------------------------------------------------------------------+
| (o) | [< Captures 2 new]  stripe-dev . Configure / Rules / Mark paid orders                  (*) Live              |
+-----+--------------------------+----------------------------------------------+------------------------------------+
| [I] | RULES        [+ New v]   | Mark paid orders           [On] [...]        | TEST  [POST /stripe . 12s      v]  |
| [P] | Top to bottom            | [Build]  Runs 48                             | [Run] [Step >] [Reset]  Auto [on]  |
| [S] | :: 1 Mark paid orders    | WHEN  [x] HTTP   [ ] Email   [ ] DNS         | ran 2 s ago . matched . 9 ms       |
|     |      HTTP . 3 steps  On  | IF [all v] of                                | ---------------------------------- |
|     | :: 2 Reject bad sign..   | [Header v][stripe-signature][exists v] x     | v Conditions          matched 2/2  |
|     |      HTTP . script   On  | [JSON v][$.type   ][= v][payment_int..] x    |   header stripe-signature  exists  |
|     |      ! 3 failed today    | [+ Condition]                                |   $.type = "payment_int.."  equal  |
|     | :: 3 Mail to chat        | THEN                                         | v 1 Extract value      ran . 1 ms  |
|     |      Email . 2 steps Off | +- 1 Extract value ----------------- [::] -+ |   + vars.amount = 2000             |
|     |                          | | from [JSON v] [$.data.object.amount]     | | > 2 Respond                   ran  |
|     | [Test all rules]         | | into vars.[amount            ]           | |   200 . 2 headers . 41 B   [View]  |
|     |                          | +------------------------------------------+ | > 3 Send request        simulated  |
|     |                          |                (+) add step                  |   POST https://erp.dev/paid [View] |
|     |                          | +- 2 Respond ----------------------- [::] -+ | ---------------------------------- |
|     |                          | | 200 . application/json . 41 B    [Edit]  | | VARIABLES after 1   [/ Filter    ] |
|     |                          | +------------------------------------------+ | request.method      "POST"         |
|     |                          | +- 3 Send request ------------------ [::] -+ | request.json.type   "payment_.."   |
|     |                          | | only if vars.amount > 0                  | | vars.amount         2000      new  |
|     |                          | | POST https://erp.dev/paid        [Edit]  | | ---------------------------------- |
|     |                          | +------------------------------------------+ | Sender gets 200 . 41 B     [View]  |
|     |                          |                (+) add step                  | [Run for real...]                  |
|     |                          | ON MATCH  (o) Next rule   ( ) Stop here      |                                    |
|     |                          | Unsaved changes        [Discard] [Save]      |                                    |
+-----+--------------------------+----------------------------------------------+------------------------------------+
```

Medium (1024 px): the rule list becomes a select; steps collapse to one line each (`[v]` expands); the
tester is a resizable bottom panel showing the trace as a breadcrumb row until expanded.

```
+----------------------------------------------------------------------------------------------+
| [=] [< Captures 2 new]  stripe-dev . Configure / Rules                           (*) Live    |
+----------------------------------------------------------------------------------------------+
| Rule [1 Mark paid orders  On v] [+ New v]  [Build] Runs 48  [On] [...]  [Test Ctrl+Enter]    |
+----------------------------------------------------------------------------------------------+
| WHEN [x] HTTP [ ] Email [ ] DNS    IF [all v] of                                             |
|   [Header v] [stripe-signature        ] [exists v]                                       [x] |
|   [JSON v]   [$.type                  ] [= v] [payment_intent.succeeded         ]        [x] |
|   [+ Condition]                                                                              |
| THEN                                                                                         |
|   1 Extract value   from JSON body $.data.object.amount into vars.amount          [::] [v]   |
|   2 Respond         200 . application/json . 41 B                                 [::] [v]   |
|   3 Send request    only if vars.amount > 0 . POST https://erp.dev/paid           [::] [v]   |
|   (+) add step                                                                               |
| ON MATCH (o) Next rule ( ) Stop here                           Unsaved  [Discard] [Save]     |
+----------------------------------------------------------------------------------------------+
| TEST [POST /stripe . 12s v] [Run] [Step >] [Reset] Auto [on]  matched . 9 ms [Resize] [Hide] |
| Conditions 2/2 > 1 Extract ran > 2 Respond ran > 3 Send request simulated      Variables [v] |
+----------------------------------------------------------------------------------------------+
```

Narrow (360 px): rule list page, then `[Build] [Test] [Runs n]` segments; reorder by `[^] [v]` buttons.

```
+----------------------------------+  +----------------------------------+  +----------------------------------+
| [<] Rules             [+ New]    |  | [<] Mark paid orders   [Save]    |  | [<] Mark paid orders   [Save]    |
+----------------------------------+  +----------------------------------+  +----------------------------------+
| Top to bottom                    |  | [Build]  Test   Runs 48          |  |  Build  [Test]  Runs 48          |
| 1 Mark paid orders         [On]  |  +----------------------------------+  +----------------------------------+
|   HTTP . 3 steps     [^] [v] >   |  | WHEN [x] HTTP [ ] Email [ ] DNS  |  | [POST /stripe . 12s         v]   |
| 2 Reject bad signature     [On]  |  | IF [all v] of                    |  | [Run] [Step >] [Reset]           |
|   HTTP . script      [^] [v] >   |  | Condition 1                      |  | matched . 9 ms . Auto [on]       |
|   ! 3 failed today               |  | [Header v] [exists v]            |  | v Conditions        matched 2/2  |
| 3 Mail to chat            [Off]  |  | [stripe-signature        ] [x]   |  | v 1 Extract value          ran   |
|   Email . 2 steps    [^] [v] >   |  | Condition 2 ...                  |  |   + vars.amount = 2000           |
| [Test all rules]                 |  | [+ Condition]                    |  | > 2 Respond                ran   |
+----------------------------------+  | THEN                             |  | > 3 Send request      simulated  |
                                      | 1 Extract value          [v]     |  | [Variables 14]                   |
                                      | 2 Respond                [v]     |  | Sender gets 200 . 41 B  [View]   |
                                      | 3 Send request           [v]     |  | [Run for real...]                |
                                      |   only if vars.amount > 0        |  +----------------------------------+
                                      | (+) add step                     |
                                      | On match [Next rule        v]    |
                                      +----------------------------------+
```

Runs tab (per rule):

```
+------------------------------------------------------------------------------+
| Mark paid orders    Build  [Runs 48]       [Failed only] [All triggers v]    |
+------------------------------------------------------------------------------+
| TIME        CAPTURE               OUTCOME           STEPS  DURATION          |
| 14:02:11    POST /stripe  c_8f2k  matched           3/3    9 ms      [Open]  |
| 13:59:40    POST /stripe  c_41aa  failed at step 3  2/3    5.0 s     [Open]  |
| 13:58:02    GET  /health  c_77b1  not matched       0/3    1 ms      [Open]  |
| [Load more]                                                                  |
+------------------------------------------------------------------------------+
```

### 17.3 Builder interaction

| Action | Pointer | Keyboard | Result |
|---|---|---|---|
| New rule | `[+ New v]`: Blank, From script, Duplicate selected | Same menu button | Opens editor with draft; nothing is saved until Save |
| Enable / disable | Switch on card and in editor header | `Space` on the focused switch | Saved immediately; Undo toast; off cards show "Off" in `text-muted`, never only dimmed |
| Reorder rules | Drag `::` handle | `Alt+Shift+Up/Down` on a focused card, or card menu Move up, Move down, Move to top | Order saved immediately (optimistic, reverted with a toast on failure); announced "Mark paid orders moved to position 2 of 3" |
| Add condition | `[+ Condition]` | Same button | New empty row focused on subject; incomplete rows are marked, not removed |
| Add step | `(+) add step` between cards and at the end | Same buttons | `AddStepMenu`: catalogue grouped by category with a filter; the new card is expanded and its first field focused |
| Edit step | Click the card summary | `Enter` on the card heading | Expands; several cards may be open; `Esc` collapses the focused one |
| Move step | Drag `[::]` | `Alt+Shift+Up/Down` | Re-validated; a "before response" step moved after a sending step gets a field error |
| Insert variable | `[{}]` next to template fields, or click a variable in the tester | Picker `Enter` | Inserted at the cursor of the last focused template field |
| Delete step or rule | Card menu Delete | `Delete` on the focused card | Removed from the draft with Undo toast (rule delete: confirm dialog, then saved) |
| Save | Save bar | `Mod+S` | Server validates; field errors attach to their card and field; the first error card expands and gets focus |
| Discard | Save bar | - | Returns to the saved version after confirm |

| Card state | Visual |
|---|---|
| Collapsed | Title, step number, one-line summary from the server's summary template, guard line |
| Expanded | Fields from schema, "Produces vars.x", phase badge |
| Invalid | `danger` left bar, error count in the title, field messages under fields |
| Highlighted by tester | `accent-soft` fill while its trace entry is selected |
| Dragging | Elevation 1, placeholder slot of the same height; reduced motion: no lift animation |

### 17.4 Test runner

**Decision: a test run executes the whole draft on the server and returns a full trace; stepping walks
that trace in the browser.** No server-side debug sessions: runs are deterministic, cheap and stateless.
Cost: variables cannot be edited mid-run; change the draft or the capture and run again.

| Topic | Rule |
|---|---|
| Input | The current draft (saved or not) plus a capture from `CapturePicker`; default `Latest` of a kind the trigger accepts; `?capture=` pins it; empty endpoint offers `[Send a test request]` |
| Run | `[Run]` or `Mod+Enter` runs from the top; the result is shown with the first entry selected |
| Step | `[Step >]` or `]` reveals the next trace entry; `[` goes back; `[Reset]` hides all but conditions. Entries after the cursor are drawn as "not reached yet" |
| Auto-run | On by default (preference); re-runs 800 ms after the last valid edit; never while the draft has errors; auto-runs are not announced |
| Ordering | Each run carries a sequence number; earlier responses arriving late are dropped; a new run aborts the previous one |
| Staleness | The result keeps the draft hash it ran on; any edit marks it `Out of date` until the next run |
| Conditions entry | First trace entry: matched / not matched, then each predicate with the actual left value, the operator and the expected value, and its result. Answers "why did my rule not fire" without guessing |
| Step entry | Outcome (ran, skipped by guard, failed, simulated, not reached), duration, variable changes (`+` new, `~` changed), output summary, `[View]` for the full output, script log lines, error with position (`[Go]` moves the cursor into the script) |
| Side effects | A test run never sends requests, emails or notifications and never changes stored data: such steps are `simulated` and `[View]` shows exactly the request or message that would go out |
| Run for real | `[Run for real...]` (editor role) runs the saved rule on the chosen capture with side effects after a confirm that lists them; recorded in the run log as manual |
| Variables inspector | Variables at the selected entry; `+` / `~` markers against the previous entry; filter; long values truncated at 200 characters with expand; click inserts the variable path into the last focused template field |
| Final result | "Sender gets" status, headers, body when any step shapes the response, else the endpoint's configured response |
| Link to builder | Selecting an entry highlights and scrolls to its card; focusing a card selects its entry when a result exists |
| Test all rules | From the rule list: runs every enabled rule in order (saved versions plus the open draft in place of its saved rule) and shows which matched, where a `Stop here` cut the chain, and the final response; each row opens that rule's trace |

Shipped (F22, F19b): the runner shows the server's trace entries (rule matched, step outcome, detail, output) and
the final status. The conditions entry with evaluated values, the variables inspector and "Sender gets" need
optional `conditions[]`, `vars` and `response` in the dry-run answer, which the server does not return yet; the
panels stay hidden until it does. A dry run never sends anything (api.md section 15), so outbound steps show as
simulated. `[Run for real...]` replays the capture to the endpoint through the composer (`POST /send` with
`endpoint_id` and `source_capture_id`), so the rules run on a new capture. Opening a recorded run read-only and
`?step=` are not built yet. The editor shows Build and Test side by side on wide screens and as tabs on
narrower ones; Test all rules opens with `?test=all` on the rules section.

```
+------------------------------------+
| TEST ALL  [POST /stripe . 12s  v]  |
| [Run]                  ran 1 s ago |
| 1 Mark paid orders   matched  >    |
| 2 Reject bad sign..  matched  >    |
|   Stop here: later rules skipped   |
| 3 Mail to chat       not run       |
| Sender gets 401 (rule 2)  [View]   |
+------------------------------------+
```

| Trace outcome | Colour token | Icon | Text |
|---|---|---|---|
| ran | `success` | check | "ran" |
| failed | `danger` | x | "failed: <reason>" |
| simulated | `info` | send | "simulated" |
| skipped by guard | `text-muted` | skip | "skipped: <guard> false" |
| not reached | `text-muted` | dot | "not reached" |
| conditions not matched | `warning` | minus | "not matched" |

### 17.5 Script step

| Topic | Rule |
|---|---|
| Editor | `ScriptEditor`: `CodeEditor` with the server language's highlighting (official CodeMirror package if one exists for that language, else a small `StreamLanguage` tokenizer for keywords, strings, numbers and comments), line numbers, bracket matching, indent on Tab |
| Help | Completion of functions and variables from the language catalogue with signature and one-line doc; hover shows the same; `[Language reference]` opens the API reference anchor in a new tab |
| Diagnostics | Server validation on save and in every test run; positions map to lint markers; runtime errors in a run jump to their line |
| Limits | Instance limits on script size and run time shown under the editor ("4.1 KiB of 32 KiB . runs stop after 2 s") |

### 17.6 Run log

Capture detail, new sections (after Forward):

```
+--------------------------------------------------------------------------------+
| RESPONSE SENT        200 . application/json . 41 B . changed by rule 1  [View] |
| RULES                                                   2 ran . 1 failed       |
| 1 Mark paid orders       matched . 3 steps . 9 ms       [Open in tester]       |
|   3 Send request  failed: timeout after 5 s                                    |
| 2 Reject bad signature   not matched (condition 1 false)                       |
| 3 Mail to chat           not run: trigger is Email                             |
+--------------------------------------------------------------------------------+
```

| Place | Content |
|---|---|
| Capture section `Response sent` | Shown when the response differed from a literal configuration (template or rule); status, headers, body as sent; "changed by rule n" link |
| Capture section `Rules` | One row per enabled rule at capture time: outcome, steps, duration, first failure; `[Open in tester]` opens the rule editor with this capture and this run's recorded trace (read-only banner "Recorded run"; `[Run again]` switches to a test run) |
| Rule tab `Runs` | Newest first, paged; filter failed only; live: new runs prepend with the arrival fade |
| Rule list | Danger line "n failed today" on cards with recent failures |

Shipped (F23): the capture `Rules` section (shown when the capture has runs), live `rule_result` updates, `[Run
again]` (re-run), `[Open in tester]` links and a Runs tab that lists the endpoint's runs (`GET .../rule-runs`).
Not built: the per-rule Runs filter (the server lists runs per endpoint), "n failed today", the recorded-run view,
and "Run again" for a capture with no runs.

## 18. Email and DNS captures

Addresses popover (`[Addresses n]` in the header, also the empty state shows all of them with a sample
`curl`, a mail command line and a `dig` line):

```
+--------------------------------------------------------------------+
| ADDRESSES                                                          |
| HTTP   https://hooks.example/k7Qm2xTa9vNc           [Copy] [curl]  |
| Alias  https://hooks.example/a/stripe-dev           [Copy] [curl]  |
| Email  k7qm2xta9vnc@in.hooks.example                [Copy]         |
|        any-name+tag@k7qm2xta9vnc.in.hooks.example is also caught   |
| DNS    k7qm2xta9vnc.dns.hooks.example               [Copy] [dig]   |
|        any-label.k7qm2xta9vnc.dns.hooks.example is also caught     |
| [Manage addresses]                                                 |
+--------------------------------------------------------------------+
```

| Kind | Badge (section 25) | Row line 1 | Row line 2 (narrow) |
|---|---|---|---|
| HTTP | Method badge | Path with query | Size, sender IP |
| Email | `MAIL` | Subject (or "(no subject)" in italics) | From address, size, attachment count |
| DNS | `DNS` | Type and query name, middle-truncated | Resolver address, transport |

### 18.1 Email detail

| Section | Content |
|---|---|
| Summary | Subject, From, To, Cc, Reply-To, Date header and received time, size, delivering host and address, TLS, authentication results when the server reports them |
| Text | Plain-text part in the body viewer (wrap on), or "No plain-text part" |
| HTML | Sandboxed frame (18.2); hidden when there is no HTML part |
| Attachments | Name, type, size, inline marker; `[Download]` always; `[Preview]` for raster images only, through the existing blob rule |
| Headers | Key-value table, original order, folded lines unfolded |
| Raw | Message source as text (body viewer, large-body rules apply), `[Download .eml]` |

Wide:

```
+---------------+-----------------------------------------------------------------------------------------------+
| webhooks.live | https://hooks.example/k7Qm2xTa9vNc [Copy] [Addresses 3]       (*) Live  [Rules 3] [Configure] |
+---------------+----------------------------------------+------------------------------------------------------+
| INBOX     14  | [/ Search    ] [Kind: all v] [Filters] | MAIL  Reset your password             [Copy v]       |
| FOLDERS       | * MAIL   Invoice 2026-11      2s       | From  "Shop" <no-reply@shop.example>                 |
| v Payments    |   POST   /stripe   1.4K      12s       | To    k7qm2xta9vnc@in.hooks.example                  |
|   stripe-dev  | > MAIL   Reset your password  4m       | Date  21 Nov 14:00:02 (4 min) . 38 KiB               |
|   paypal-sb   |   DNS    A x1.k7qm..dns..      5m      | Via   mx.shop.example 192.0.2.10 . TLS               |
| v Unfiled     |   GET    /health   0 B        5m       | Summary  Text  HTML  Attachments 2  Headers  Raw     |
|   github-app  |                                        | --------------------------------------------------   |
|               |                                        | HTML  Remote images blocked [Load] [375|600|Fit]     |
|               |                                        | +------------------------------------------------+   |
|               |                                        | | (sandboxed frame, white canvas, no scripts)    |   |
|               |                                        | |  SHOP                                          |   |
|               |                                        | |  Reset your password                           |   |
|               |                                        | |  [ Reset password ]   (links are inert)        |   |
|               |                                        | +------------------------------------------------+   |
|               |                                        | [Open full view]                                     |
|               |                                        | ATTACHMENTS                                          |
|               |                                        | invoice.pdf     application/pdf   112 KiB [Download] |
|               |                                        | logo.png        image/png  inline   4 KiB [Preview]  |
+---------------+----------------------------------------+------------------------------------------------------+
```

Medium:

```
+----------------------------------------------------------------------------------------------+
| [=] webhooks.live  https://hooks.example/k7Qm2xTa9vNc [Copy]        (*) Live  [...]          |
+--------------------------------+-------------------------------------------------------------+
| [/ Search   ] [Filters 1]      | MAIL  Reset your password                       [Copy v]    |
| * MAIL Invoice 2026-11  2s     | From "Shop" <no-reply@shop.example> . 14:00:02 . 38 KiB     |
|   POST /stripe  1.4K   12s     | Summary  Text  HTML  Attachments 2  Headers  Raw            |
| > MAIL Reset your pas.. 4m     | HTML   Remote images blocked  [Load]   [375|600|Fit]        |
|   DNS  A x1.k7qm..      5m     | +------------------------------------------------------+    |
|                                | | (sandboxed frame)                                    |    |
|                                | +------------------------------------------------------+    |
+--------------------------------+-------------------------------------------------------------+
```

Narrow (list, then detail):

```
+------------------------------------+    +------------------------------------+
| [=] webhooks.live (*) Live [...]   |    | [<] MAIL Reset your pa.. [Copy v]  |
+------------------------------------+    +------------------------------------+
| k7Qm2x...vNc [Copy] [Addresses]    |    | From no-reply@shop.example         |
| [/ Search     ] [Filters 1]        |    | 14:00:02 (4 min) . 38 KiB          |
| * MAIL Invoice 2026-11       2s    |    | [Summary][Text][HTML][Files 2]     |
|   billing@shop.example . 41 KiB    |    | Images blocked [Load] [Fit v]      |
| > MAIL Reset your password   4m    |    | +------------------------------+   |
|   no-reply@shop.example . 38 KiB   |    | | (sandboxed frame, 360 wide,  |   |
|   DNS  A x1.k7qm2x...        5m    |    | |  scrolls inside)             |   |
|   198.51.100.53 . UDP              |    | +------------------------------+   |
+------------------------------------+    | [Open full view]                   |
                                          | [< Newer]    3 / 312   [Older >]   |
                                          +------------------------------------+
```

### 18.2 Email HTML sandbox (risky)

| Rule | Detail |
|---|---|
| Frame | `<iframe sandbox="">` with no allow tokens: no scripts, no same origin, no forms, no popups, no top navigation |
| Source | `src` is a backend route serving the HTML part with `Content-Security-Policy: sandbox; default-src 'none'; style-src 'unsafe-inline'; img-src data: <same-origin attachment route>; frame-ancestors 'self'`, `nosniff`, `text/html; charset=utf-8`; never `srcdoc` (the parent page's CSP would apply and the HTML would pass through React state) |
| Interface CSP | Needs `frame-src 'self'` added to the UI policy (architecture section 9); nothing else is loosened |
| Inline images | `cid:` references rewritten by the server to the attachment route |
| Remote images | Blocked by default; `[Load]` reloads the frame with a server flag that permits `img-src https:` for this view only, after a one-line warning "The sender can see that the message was opened, and from which address" |
| Links | Inert (no popups allowed); hovering shows nothing. Rejected: `allow-popups`, which would make phishing links one click away |
| Size | Fixed height 480 px (wide), 360 px (narrow), drag handle to resize, inner scroll; width presets `375`, `600`, `Fit`; canvas `frame-canvas` in both themes |
| Full view | `/e/:id/c/:captureId/html`: the same frame filling the viewport, top bar with Back and the same controls |
| Never | `dangerouslySetInnerHTML`, `DOMParser` rendering, or copying the HTML into the interface document |

### 18.3 DNS detail

| Section | Content |
|---|---|
| Summary | Query name, type with number, class, the label part before the endpoint's DNS name, asking address and transport, received time |
| Query | Flags, EDNS options, question section |
| Answer | What the server answered (records, TTL) or "No answer configured" |
| Raw | `HexView` of the packet, `[Download]` |

Wide (list and detail):

```
+---------------+----------------------------------------+-----------------------------------------------------+
| INBOX     14  | [/ Search    ] [Kind: DNS v] [Filters] | DNS  A  x1.k7qm2xta9vnc.dns.hooks.example  [Copy v] |
| FOLDERS       | * DNS    TXT _acme.k7qm..     2s       | Asked by    198.51.100.53 (resolver) . UDP          |
| v Payments    | > DNS    A   x1.k7qm2x..dns.. 5m       | When        21 Nov 13:57:30 (5 min)                 |
|   stripe-dev  |   DNS    AAAA x1.k7qm2x..     5m       | Question    x1.k7qm2xta9vnc.dns.hooks.example       |
|   paypal-sb   |   POST   /stripe   1.4K      12s       | Type        A (1)   Class IN                        |
| v Unfiled     |                                        | Label       x1   (the part before your name)        |
|   github-app  |                                        | Answered    A 203.0.113.80 . TTL 60                 |
|               |                                        | Flags       RD . EDNS0 4096 . DO                    |
|               |                                        | Summary  Query  Answer  Raw                         |
|               |                                        | RAW PACKET  62 B                  [Download]        |
|               |                                        | 0000  8a 1f 01 00 00 01 00 00 00 00 00 01 02 78     |
+---------------+----------------------------------------+-----------------------------------------------------+
```

Medium:

```
+--------------------------------+------------------------------------------------------------+
| [/ Search   ] [Kind: DNS v]    | DNS  A  x1.k7qm2xta9vnc.dns.hooks.example      [Copy v]    |
| * DNS TXT _acme.k7qm..  2s     | 198.51.100.53 (resolver) . UDP . 13:57:30 (5 min)          |
| > DNS A   x1.k7qm2x..   5m     | Type A (1) . Class IN . Label x1                           |
|   DNS AAAA x1.k7qm2x..  5m     | Answered A 203.0.113.80 . TTL 60                           |
|                                | Summary  Query  Answer  Raw                                |
+--------------------------------+------------------------------------------------------------+
```

Narrow:

```
+------------------------------------+
| [<] DNS A x1.k7qm2x...   [Copy v]  |
+------------------------------------+
| 13:57:30 (5 min) . UDP             |
| [Summary][Query][Answer][Raw]      |
| Question                           |
| x1.k7qm2xta9vnc.dns.hooks.example  |
| Type   A (1)      Class IN         |
| Label  x1                          |
| From   198.51.100.53 (resolver)    |
| Answer A 203.0.113.80 . TTL 60     |
| [< Newer]    2 / 312   [Older >]   |
+------------------------------------+
```

## 19. Combined inbox, folders, labels, my endpoints

**Decision: the inbox is the endpoint view with a scope instead of one endpoint.** Same list, detail,
follow, filters and shortcuts; each row adds an endpoint chip, and actions on a row resolve against that
row's endpoint and role. Rejected: a separate dashboard screen (a second list to keep in sync).

Wide:

```
+---------------+-----------------------------------------------------------------------------------------------+
| webhooks.live | INBOX  [All my endpoints v]  14 unread across 3            (*) Live   [Follow: off]  [Export] |
+---------------+------------------------------------------------+----------------------------------------------+
| > INBOX   14  | [/ Search all   ] [Endpoint v] [Kind v] [1h] x | POST /hook . github-app      [Open] [Copy v] |
| FOLDERS       |           ^ 2 new                              | 192.30.252.1 . 14:00:09 (2 min) . 6.1 KiB    |
| v Payments    | * POST  /stripe        [stripe-dev]       12s  | Summary  Rules  Query  Body  Headers 22  Raw |
|  stripe-dev 9 | * MAIL  Invoice 2026-11 [stripe-dev]       1m  | {                                            |
|  paypal-sb    | > POST  /hook          [github-app]        2m  |   "action": "opened",                        |
| v Unfiled     |   DNS   A x1.k7qm..    [stripe-dev]        5m  |   "number": 42                               |
|  github-app 5 |   GET   /ping          [paypal-sb]         9m  | }                                            |
|               | |< < > >|         1-50 of 128                  |                                              |
+---------------+------------------------------------------------+----------------------------------------------+
```

Medium:

```
+----------------------------------------------------------------------------------------------+
| [=] INBOX [All my endpoints v]  14 unread                (*) Live  [Follow]  [...]           |
+------------------------------------+---------------------------------------------------------+
| [/ Search     ] [Filters 2]        | POST /hook . github-app       [Open endpoint] [Copy v]  |
| * POST /stripe  stripe-dev 12s     | 192.30.252.1 . 14:00:09 (2 min) . 6.1 KiB               |
| * MAIL Invoice  stripe-dev  1m     | { "action": "opened", "number": 42 }                    |
| > POST /hook    github-app  2m     |                                                         |
|   DNS  A x1..   stripe-dev  5m     |                                                         |
+------------------------------------+---------------------------------------------------------+
```

Narrow:

```
+------------------------------------+
| [=] INBOX (*) Live          [...]  |
| [All my endpoints           v]     |
| [/ Search     ] [Filters 2]        |
+------------------------------------+
| * POST /stripe               12s   |
|   stripe-dev . 1.4 KiB             |
| * MAIL Invoice 2026-11        1m   |
|   stripe-dev . billing@shop...     |
| > POST /hook                  2m   |
|   github-app . 6.1 KiB             |
+------------------------------------+
```

| Topic | Rule |
|---|---|
| Scope | All my endpoints (local plus account), a folder, or a label; `[Endpoint v]` narrows further inside the scope |
| Live | One multiplexed stream for the scope, not one `EventSource` per endpoint; reconnect catch-up per endpoint by `seq` as in section 6 |
| Unread | Per endpoint counts in the rail; the inbox total is their sum |
| Detail | Header line shows the endpoint label and `[Open endpoint]`; deep link `/inbox/:endpointId/:captureId` |
| Empty | "No captures in this scope yet", list of the scope's addresses with Copy |
| Folders | One level; an endpoint is in at most one folder; create, rename and delete from the rail context menu; delete moves endpoints to Unfiled |
| Labels | Free-text chips, many per endpoint, no colours (no new tokens, nothing colour-only) |
| My endpoints page | Table: label, id, source (`This browser`, `Account`, `Shared, <role>`), folder, labels, captures, last activity; filters by folder, label, source; `[Move to account]` on local rows when signed in; Remove from list and Delete separated |

## 20. Composer and scheduled requests

### 20.1 Composer

Requests are sent by the server (same SSRF guard and error codes as forwarding), never by the browser.

Wide:

```
+---------------+-----------------------------------------------------------------------------------------------+
| webhooks.live | COMPOSER                                                            [History 12 v]            |
+---------------+-----------------------------------------------------+-----------------------------------------+
| INBOX     14  | [POST v] [https://erp.dev/orders/77/paid   ] [Send] | RESPONSE  200 . 143 ms . 96 B           |
| FOLDERS       | Replay of POST /stripe from stripe-dev     [Clear]  | final https://erp.dev/orders/77/paid    |
| v Payments    |  Params 1  [Headers 6]  Body  Options               | [Body]  Headers 7  Timing               |
|   stripe-dev  | HEADERS                                [Bulk edit]  | {                                       |
|   paypal-sb   | [x] content-type     [application/json         ]    |   "ok": true,                           |
| v Unfiled     | [x] stripe-signature [t=1732...,v1=9a..        ]    |   "order": "77"                         |
|   github-app  | [ ] user-agent       [Stripe/1.0               ]    | }                                       |
|               | [+ Header]                                          |                                         |
|               | Not sent: host, content-length (set by the server)  | Sent from the server, not your browser. |
|               | BODY   [application/json v]          [Pretty|Raw]   | Private and local targets are refused.  |
|               | 1  {                                                |                                         |
|               | 2    "id": "evt_1Q",                                |                                         |
|               | 3    "type": "payment_intent.succeeded"             |                                         |
|               | 4  }                                                |                                         |
|               | 1.4 KiB                     [Save as schedule...]   |                                         |
+---------------+-----------------------------------------------------+-----------------------------------------+
```

Medium (request above, response below, resizable):

```
+----------------------------------------------------------------------------------------------+
| [=] COMPOSER                                                              [History 12 v]     |
+----------------------------------------------------------------------------------------------+
| [POST  v] [https://erp.dev/orders/77/paid                                         ] [Send]   |
| Replay of POST /stripe from stripe-dev [Clear]       Params 1  [Headers 6]  Body  Options    |
| [x] content-type     [application/json                        ]                              |
| [x] stripe-signature [t=1732...,v1=9a..                       ]               [+ Header]     |
+----------------------------------------------------------------------------------------------+
| RESPONSE 200 . 143 ms . 96 B . final URL https://erp.dev/orders/77/paid   [Body] Headers 7   |
| { "ok": true, "order": "77" }                                                                |
+----------------------------------------------------------------------------------------------+
```

Narrow (`[Request] [Response]` segments):

```
+------------------------------------+    +------------------------------------+
| [<] Composer          [History]    |    | [<] Composer          [History]    |
+------------------------------------+    +------------------------------------+
| [Request]  Response 200            |    |  Request  [Response 200]           |
+------------------------------------+    +------------------------------------+
| [POST v] [Send]                    |    | 200 . 143 ms . 96 B                |
| [https://erp.dev/orders/77/paid]   |    | https://erp.dev/orders/77/paid     |
| Replay of POST /stripe [Clear]     |    | [Body]  Headers 7  Timing          |
| [Params 1 v]                       |    | {                                  |
| [Headers 6 v]                      |    |   "ok": true,                      |
|   content-type: application/json   |    |   "order": "77"                    |
|   stripe-signature: t=1732...      |    | }                                  |
| [Body v]  application/json         |    +------------------------------------+
| { "id": "evt_1Q", ...              |
+------------------------------------+
```

| Topic | Rule |
|---|---|
| Request editor | `RequestEditor` shared with schedules: method (free token allowed), URL, Params table synced with the URL, Headers table with enable checkboxes and bulk edit, Body (`CodeEditor`, content type select, `none`, raw text, JSON, form fields, binary from file), Options (timeout up to the instance limit, follow redirects up to 5) |
| Replay prefill | From a capture: method, captured path and query appended to an empty target, headers without hop-by-hop, `host`, `content-length` and the forward denylist, raw body; banner "Replay of POST /stripe from stripe-dev [Clear]" |
| Default target | Empty; quick pick menu offers the current endpoint's capture URL and the last 5 targets |
| Send | `[Send]` or `Mod+Enter`; while sending `[Cancel]`; response view reuses `BodyViewer`, `KeyValueTable`, timing (DNS, connect, TLS, first byte, total when reported) |
| Errors | SSRF refusals and connection errors show on the URL field with the server reason, like forwarding |
| History | Last 20 sends in local storage (request only, no response bodies), `[History v]` restores one |
| To schedule | `[Save as schedule...]` asks for an endpoint (default: replay source) and opens its schedule editor with the request |

### 20.2 Scheduled requests

Wide (workspace section; medium uses the tab strip; narrow: list page, then editor page with History below):

```
+--------------------+------------------------------+----------------------------------------------------------+
| CONFIGURE          | SCHEDULES      [+ New]       | Ping staging                  [Active] [Pause]           |
|   General          | > Ping staging    Active     | REQUEST  (same editor as the composer)                   |
|   Response         |   every 5 min . next 14:05   | [GET  v] [https://staging.example/health     ]           |
|   Rules          3 |   last 200 . 2 min ago       | CADENCE                                                  |
|   Forwarding     1 |   Nightly report  Paused     | (o) Every [5    ] [minutes v]                            |
|   Access           |   daily 02:00 UTC            | ( ) Daily at [02:00] [UTC          v]                    |
|   Notifications    |   last 500 . 3 d ago         | ( ) Expression [*/5 * * * *          ]                   |
| > Schedules      2 |                              | Next: 14:05, 14:10, 14:15, 14:20, 14:25                  |
|   Local forwarding |                              | On failure  [x] Notify [ops-chat v]                      |
|                    |                              | HISTORY                            [Failed only]         |
|                    |                              | 14:00:00   200   88 ms                  [Open]           |
|                    |                              | 13:55:00   503   1.2 s                  [Open]           |
|                    |                              |                     [Delete]  [Discard] [Save]           |
+--------------------+------------------------------+----------------------------------------------------------+
```

| Topic | Rule |
|---|---|
| Ownership | A schedule belongs to an endpoint; history entries link to captures when the target is a capture URL of the same instance |
| Cadence | Every n minutes or hours (instance minimum), daily at a time with time zone, or an expression; the server returns the next 5 run times, shown under the field; invalid expressions are field errors |
| State | Active or Paused; pausing applies at once; schedules of a cloned endpoint start paused |
| History | Time, status, duration, `[Open]` shows the request and response like the composer; failed only filter |
| On failure | Optional notification channel (section 22.5) |

## 21. Export and import

**Decision: multi-capture export is a server job; single-capture copy-as stays client-side (section 5).**
The client never holds all captures of a filter, ZIP needs raw bodies and attachments, and a job survives
closing the dialog. Rejected: building bulk files in the browser (memory, partial pages, no progress truth).

Export dialog, wide and medium (centred dialog 560 px):

```
+--------------------------------------------------------------+
| Export captures                                         [x]  |
+--------------------------------------------------------------+
| WHAT                                                         |
| ( ) Selected                           3                     |
| (o) Matching current filters          48   method POST, 1h   |
| ( ) All captures of stripe-dev       312                     |
| FORMAT                                                       |
| (o) JSON   one file, full captures                           |
| ( ) CSV    one row per capture, summary columns              |
| ( ) HAR    one log, HTTP captures only (emails, DNS skipped) |
| ( ) ZIP    one folder per capture with raw body and files    |
| [x] Include bodies (adds up to 14.6 MiB)                     |
+--------------------------------------------------------------+
|                                     [Cancel]  [Export 48]    |
+--------------------------------------------------------------+
```

Running and done:

```
+----------------------------------------------+    +----------------------------------------------+
| Export captures                         [x]  |    | Export captures                         [x]  |
+----------------------------------------------+    +----------------------------------------------+
| Preparing ZIP . 31 of 48 captures            |    | Ready . 48 captures . ZIP . 15.1 MiB         |
| [##################----------]  65 %         |    | Link valid until 15:10 (1 h).                |
| You can close this dialog; the export        |    | 2 captures were deleted meanwhile and        |
| keeps running and stays in Exports.          |    | are not included.                            |
+----------------------------------------------+    +----------------------------------------------+
|                         [Cancel export]      |    |                      [Close]  [Download]     |
+----------------------------------------------+    +----------------------------------------------+
```

Narrow (full-screen dialog):

```
+------------------------------------+
| [x] Export captures                |
+------------------------------------+
| WHAT                               |
| ( ) Selected                    3  |
| (o) Matching filters           48  |
| ( ) All                       312  |
| FORMAT                             |
| (o) JSON  one file                 |
| ( ) CSV   one row per capture      |
| ( ) HAR   HTTP only                |
| ( ) ZIP   bodies and files         |
| [x] Include bodies (14.6 MiB)      |
+------------------------------------+
| [Cancel]              [Export 48]  |
+------------------------------------+
```

| Format | Content |
|---|---|
| JSON | One file: array of full captures of all kinds, bodies base64 with content type |
| CSV | One row per capture: id, kind, received time, method or kind, path or subject or query name, sender, size, content type, response status; bodies never |
| HAR | One HAR 1.2 log of the HTTP captures; email and DNS are skipped and counted |
| ZIP | `captures.json` plus one folder per capture with `request.json`, `body.bin`, email `message.eml` and attachments, DNS `packet.bin` |

| Topic | Rule |
|---|---|
| Scope | Selected (select mode), matching current filters, all; counts from the server |
| Progress | Polled every 2 s or pushed on the stream, `n of m`; cancel stops the job |
| Done | Download link with its expiry; captures deleted meanwhile are reported |
| Recent exports | `Exports` list in the `[...]` menu shows the last jobs of this endpoint until their links expire |

Import dialog:

```
+--------------------------------------------------------------+
| Import captures                                         [x]  |
+--------------------------------------------------------------+
| +--------------------------------------------------------+   |
| |   Drop a file here or [Choose file]                    |   |
| |   webhooks.live JSON or ZIP export, or a HAR log       |   |
| +--------------------------------------------------------+   |
| stripe-dev-export.json . 2.1 MiB . 48 HTTP, 2 email          |
| INTO   (o) stripe-dev (312 / 500)   ( ) a new endpoint       |
| HOW    (o) Store as captures, keep original times            |
|        ( ) Send again to the capture URL (rules run)         |
| 2 entries exceed 10 MiB and will be skipped.     [Details]   |
+--------------------------------------------------------------+
|                                     [Cancel]  [Import 48]    |
+--------------------------------------------------------------+
```

| Topic | Rule |
|---|---|
| Accepted | webhooks.live JSON or ZIP export, HAR 1.2 |
| Preview | Parsed by the server; counts per kind, entries that will be skipped and why |
| Modes | Store as captures with original times (no rules run) or send again to the capture URL (new captures, rules run, rate limits apply) |
| Target | This endpoint (capacity shown) or a new endpoint |
| Result | Imported, skipped with reasons, link to the endpoint |

## 22. Account area

### 22.1 Account menu and profile

| Item | Content |
|---|---|
| Account button | Initials in a 32 px circle (`surface-sunken`, `text`), name on wide; anonymous with sign-in available: `[Sign in]` |
| `/account` | Name and email from the identity provider (read-only), sign-in provider, `[Sign out]`; tabs Profile, API tokens, Notifications, Usage |
| Sign out | `POST` sign-out, then home; local endpoints stay in this browser |

### 22.2 API tokens

Wide:

```
+---------------+-----------------------------------------------------------------------------------------------+
| webhooks.live | ACCOUNT  Profile  [API tokens]  Notifications  Usage                    [Create token]        |
+---------------+-----------------------------------------------------------------------------------------------+
| INBOX     14  | API TOKENS                                                                                    |
| FOLDERS       | A token acts as you in the API and the CLI, limited to its scopes and endpoints.              |
| v Payments    | NAME          SCOPES                            ENDPOINTS  EXPIRES      LAST USED             |
|   stripe-dev  | ci-runner     Captures read, Rules write        2          in 82 days   3 h ago   [Revoke]    |
|   paypal-sb   | cli-laptop    Captures read, Live stream        all        never        just now  [Revoke]    |
| v Unfiled     | old-script    Endpoints write                   all        expired      12 d ago  [Delete]    |
|   github-app  |               wl_7Kq...                         created 2 Sep by you                          |
|               |                                                                                               |
| [Account v]   |                                                                                               |
+---------------+-----------------------------------------------------------------------------------------------+
```

Medium:

```
+----------------------------------------------------------------------------------------------+
| [=] ACCOUNT  Profile  [API tokens]  Notifications  Usage                  [Create token]     |
+----------------------------------------------------------------------------------------------+
| NAME          SCOPES          ENDPOINTS   EXPIRES       LAST USED                            |
| ci-runner     2 scopes        2           in 82 days    3 h ago                    [Revoke]  |
| cli-laptop    2 scopes        all         never         just now                   [Revoke]  |
| old-script    1 scope         all         expired       12 d ago                   [Delete]  |
+----------------------------------------------------------------------------------------------+
```

Narrow:

```
+------------------------------------+
| [<] API tokens           [Create]  |
+------------------------------------+
| ci-runner                 [Revoke] |
| Captures read, Rules write         |
| 2 endpoints . expires in 82 days   |
| last used 3 h ago                  |
| ---------------------------------- |
| cli-laptop                [Revoke] |
| Captures read, Live stream         |
| all endpoints . never expires      |
+------------------------------------+
```

Create, and the one-time secret:

```
+----------------------------------------------------+    +----------------------------------------------------+
| Create token                                  [x]  |    | Token created                                 [x]  |
+----------------------------------------------------+    +----------------------------------------------------+
| Name     [ci-runner                          ]     |    | Copy it now. It is shown only once and             |
| SCOPES                  read          write        |    | cannot be recovered; create a new one if lost.     |
| Endpoints               [x]           [ ]          |    | [wl_7KqP2...hidden...Xn4                ] [Copy]   |
| Captures                [x]           [ ] delete   |    |                                   [Show]           |
| Rules                   [x]           [x]          |    | Use it as:  Authorization: Bearer <token>          |
| Send requests           -             [ ]          |    +----------------------------------------------------+
| Live stream             [x]           -            |    |                                          [Done]    |
| ENDPOINTS (o) All mine  ( ) Only [stripe-dev x][+] |    +----------------------------------------------------+
| EXPIRES   [90 days     v]   until 19 Feb 2027      |
+----------------------------------------------------+
|                            [Cancel]  [Create]      |
+----------------------------------------------------+
```

| Rule | Detail |
|---|---|
| Scopes | From the server scope catalogue, drawn as a resource by read/write grid; impossible cells are `-` |
| Endpoints | All mine, or a chosen list |
| Expiry | Options the instance allows; "Never" only if allowed |
| Secret | Shown once in a read-only mono field, masked with `[Show]`, `[Copy]`; held only in the dialog's component state, never in the query cache, URL, local storage or logs; cleared on close. Closing before Copy asks "Close without copying? The token cannot be shown again" |
| Revoke | Confirm dialog naming the token; immediate; expired tokens offer Delete instead |

### 22.3 API reference

**Decision: an own renderer for the served OpenAPI document**, as a lazy route chunk. Rejected: embedding a
third-party OpenAPI viewer (they inject `<style>` elements and scripts the interface CSP forbids, add
several hundred KB, and look foreign).

```
+--------------------------------+------------------------------------------------------------------------------+
| API REFERENCE   v1.4.0         | POST /api/v1/endpoints                                  [Open in composer]   |
| [/ Filter operations    ]      | Create an endpoint.                                                          |
| ENDPOINTS                      | AUTH          session, or a token with scope Endpoints write                 |
|   GET    /endpoints            | REQUEST BODY  application/json                                               |
| > POST   /endpoints            |   title          string, up to 100, optional                                 |
|   PATCH  /endpoints/{id}       |   response       object, optional               [Expand]                     |
| CAPTURES                       | RESPONSES                                                                    |
|   GET    .../{id}/captures     |   201  Endpoint                                  [Expand]                    |
| RULES                          |   422  validation_failed                         [Expand]                    |
|   POST   .../rules/{id}/test   | EXAMPLE                                         [curl|HTTP]   [Copy]         |
| [Download OpenAPI]             | curl --request POST --url https://hooks.example/api/v1/endpoints \           |
|                                |   --header "Authorization: Bearer $WEBHOOKS_LIVE_TOKEN" --data '{}'          |
+--------------------------------+------------------------------------------------------------------------------+
```

| Rule | Detail |
|---|---|
| Subset | OpenAPI 3.1: tags, operations, parameters, request bodies, responses, `$ref`, `oneOf` shown as alternatives, examples |
| Text | Descriptions as plain paragraphs with inline code spans; no Markdown-to-HTML |
| Samples | curl and raw HTTP, with the instance's public URL and `Authorization: Bearer $WEBHOOKS_LIVE_TOKEN`; never a real token |
| Composer | `[Open in composer]` fills method, URL and example body: the request is handed to `/send` in memory in the same tab (no URL parameters, nothing stored), and the composer consumes it once |
| Layouts | Shipped: below 1024 px the operation list is a toggled panel (drawer) above the operation, instead of the select (medium) and the list page then operation page flow (narrow) drawn here |
| Delivery | Lazy route chunk (about 11 kB gzip); operation bodies render only when expanded; `[Download OpenAPI]` links the served document; i18n area `apireference` |

### 22.4 Local forwarding (CLI)

```
+--------------------+----------------------------------------------------------------------------------+
| CONFIGURE          | LOCAL FORWARDING                                                                 |
|   ...              | Replay every capture of this endpoint to a server on your machine.               |
|   Schedules      2 | 1 INSTALL    [macOS] Linux  Windows  Container                                   |
| > Local forwarding |    brew install ...                                   (from the server)  [Copy]  |
|                    | 2 TOKEN    [Create a token for this command]  scopes: Captures read, Stream      |
|                    | 3 RUN        Forward to  [http://localhost:3000   ]                              |
|                    |    <cli> forward k7Qm2xTa9vNc --to http://localhost:3000                 [Copy]  |
|                    | Captures replay in order; the CLI resumes after a disconnect.                    |
+--------------------+----------------------------------------------------------------------------------+
```

| Rule | Detail |
|---|---|
| Install | Per platform commands and download links come from the server (they change with releases) |
| Command | Built from the server's command template with the endpoint id and the target typed in `Forward to` (default `http://localhost:3000`, remembered) |
| Token | Where tokens exist, `[Create a token for this command]` creates a token limited to this endpoint with read and stream scopes and shows it once inside an `export` line |

### 22.5 Notifications

Endpoint triggers (workspace section):

```
+--------------------+----------------------------------------------------------------------------------+
| CONFIGURE          | NOTIFICATIONS FOR stripe-dev                                                     |
|   ...              | WHEN                                   SEND TO          AT MOST                  |
| > Notifications    | [x] Every capture                      [ops-chat    v]  1 per [5 min v]          |
|   Schedules      2 | [x] Captures matching [Failed pays v]  [alex mail   v]  every time               |
|                    | [x] A rule fails                       [ops-chat    v]  1 per [15 min v]         |
|                    | [ ] Captures reach 90 % of the limit   [-           v]                           |
|                    | [ ] Endpoint expires within 24 h       [-           v]                           |
|                    | [x] This browser (desktop notification while a tab is open)   [Allow...]         |
|                    | Channels are managed in Account > Notifications.         [Manage channels]       |
|                    |                                          Unsaved changes  [Discard] [Save]       |
+--------------------+----------------------------------------------------------------------------------+
```

Channels (account; where there are no accounts, the same table sits inside the endpoint section):

```
+----------------------------------------------------------------------------------------------------+
| ACCOUNT  Profile  API tokens  [Notifications]  Usage                           [Add channel]       |
| CHANNEL        KIND          TARGET                         USED BY   LAST                         |
| ops-chat       Chat webhook  https://chat.example/hooks/... 3 rules   ok 2 min ago   [Test]        |
| alex mail      Email         alex@example.com               1 filter  ok 1 d ago     [Test]        |
| pager          HTTP POST     https://pager.example/in       none      failed: 401    [Test]        |
|                Test sent at 14:03: 401 Unauthorized. Check the token in the URL.   [Edit]          |
+----------------------------------------------------------------------------------------------------+
```

| Rule | Detail |
|---|---|
| Channel kinds | From the server catalogue with field schemas (same `SchemaField` widgets as steps) |
| Test | `[Test]` sends a test message and shows the result inline (status or error text) with time |
| Triggers | Every capture, captures matching a saved filter, a rule fails, captures at 90 % of the limit, endpoint expires within 24 h; per-rule notifications are the Notify step |
| Throttle | "At most 1 per n minutes" per trigger; extra events are counted into the next message |
| Browser | Desktop notifications are local: permission asked only after the user ticks the box; shown only while a tab is open |

Shipped (F39) and deferred:

| Part | State |
|---|---|
| Channels | Per endpoint in the workspace section Notifications: table, editor per kind, enable switch, delete, `[Test]` (the result is the queued delivery; its final state shows on captures, a delivery status route is planned). The kind list is built into the UI, not a server catalogue |
| Triggers | Deferred: the server knows only "every capture" (`trigger: capture`, no condition) and notify steps (`trigger: rule`); saved-filter, rule-failure, 90 % and expiry triggers and per-trigger throttles are planned. The throttle is the channel's `per_hour` |
| E-mail | Address list with verification; `/email/verify` and `/notifications/disable` landing pages |
| Browser push | `[Allow in this browser]` registers `/sw.js` and subscribes on that click only (never on page load); states unsupported, sign in needed, subscribed, blocked, dismissed, worker failed. An existing browser subscription is reused only when its key matches the served public key. There is no flow to remove a subscription (`DELETE /api/v1/push/subscriptions/{id}` is unused by the UI). The worker's notification text is not translated (no build step, no i18n) and shows no icon |

### 22.6 Usage and limits

```
+----------------------------------------------------------------------------------------------------+
| ACCOUNT  Profile  API tokens  Notifications  [Usage]                                               |
| USAGE AND LIMITS            Limits are set by whoever runs this instance.                          |
| WHAT                     USE              LIMIT     WHEN REACHED                                   |
| Captures per endpoint    312 (stripe-dev) 500       New captures get 410 and are not stored,       |
|                          [######----]               unless the endpoint drops the oldest.          |
| Endpoints                7                20        Creating another one is refused.               |
| Rule runs today          1 204            10 000    Rules stop running until 00:00 UTC;            |
|                          [#---------]               captures are still stored.                     |
| Emails today             38               200       Further mail is refused by the server.         |
| Scheduled requests       2                10        New schedules cannot be created.               |
| Total storage            41 MiB           1 GiB     Least recently used endpoints are removed.     |
+----------------------------------------------------------------------------------------------------+
```

| Rule | Detail |
|---|---|
| Wording | Neutral and factual: what the limit is, current use, what happens at the limit, how to get under it (delete, ring mode, wait for reset). Never offers or mentions paid upgrades. Footer: "Limits are set by whoever runs this instance" |
| Meters | `UsageMeter`: `accent` fill, `warning` at 90 %, `danger` at 100 %, value always in text |
| Placement | Account page; per-endpoint meters in General; the header's existing capacity text links to General |

### 22.7 Preferences (`/settings`)

| Group | Items |
|---|---|
| Appearance | Theme, density stays fixed |
| Language | Language control (existing) |
| Live | Follow default, toasts, screen-reader announcements, desktop notifications default |
| Editors | Indent 2 or 4, wrap, auto-run tests, autocomplete on typing, show line numbers |
| Shortcuts | Single-key shortcuts on/off, the shortcut list (same as the dialog) |
| Data | Clear local endpoint list, export and import the local endpoint list as a file |

## 23. Search and filters

Filter builder (popover on wide, bottom sheet on narrow; the chip row of section 3 shows the active parts):

```
+----------------------------------------------------------------+
| FILTERS                                   [Saved v] [Clear]    |
+----------------------------------------------------------------+
| Text   [invoice_id        ] in [Body v]                        |
| Header [stripe-signature  ] [contains v] [v1=         ]  [x]   |
| [+ Header condition]                                           |
| Kind   [x] HTTP  [x] Email  [ ] DNS                            |
| Method [POST x] [+]      Status [4xx x] [5xx x] [+]            |
| Sender [203.0.113.0/24        ]                                |
| Rules  [Any v]  (any, matched, failed, none)                   |
| Time   [Last 1 h v]   from [2026-11-21 13:00] to [now  ]       |
+----------------------------------------------------------------+
| [Save as...]                    48 match          [Apply]      |
+----------------------------------------------------------------+
```

| Parameter | Meaning |
|---|---|
| `q` | Text, case-insensitive |
| `in` | `any` (default), `body`, `headers`, `path`, `query`, `email` (subject and text part) |
| `h` | Header condition `name:op:value`, repeatable; `op` is `has`, `eq`, `contains` |
| `kind` | `http,email,dns` |
| `method`, `status` | As before |
| `ip` | Sender address or range |
| `rule` | `matched`, `failed`, `none` |
| `range` | `15m`, `1h`, `24h`, `7d`; or `since` and `until` as absolute times |
| `order` | `oldest` flips the sort |
| `saved` | Id of the saved filter last applied (label only; the filter itself is always spelled out in the URL) |

| Topic | Rule |
|---|---|
| Saved filters | Name plus parameters; stored on the server per user when signed in, else in local preferences; menu `[Saved v]` applies, renames, deletes; usable as notification triggers |
| Live | Kind, method, status, sender and time filter live summaries client-side as today; text, header and rule filters refetch the first page 1 s after an arrival |
| Large bodies | A hint states the server's searched prefix (first 256 KiB today) |
| Inbox | Same parameters, scoped |

## 24. Component inventory additions

`SettingsSheet` and `ForwardingSheet` of section 8 become `EndpointWorkspace` sections; their states carry
over to `SaveBar`.

| Component | States | Props (high level) |
|---|---|---|
| `EndpointWorkspace` | wide, medium, narrow list, narrow section | `endpointId`, `section`, `children` |
| `WorkspaceSectionNav` | side, tabs, list; badges; danger dot | `sections`, `active`, `counts` |
| `SaveBar` | hidden, dirty, saving, error, conflict | `dirty`, `saving`, `error`, `onSave`, `onDiscard` |
| `ConflictBanner` | hidden, shown | `onReload`, `onOverwrite` |
| `AddressesPopover` | closed, open | `addresses` (`kind`, `value`, `sample`) |
| `AddressRow` | default, copied, removing | `kind`, `value`, `onCopy`, `onRemove` |
| `AliasField` | idle, checking, available, taken, invalid | `value`, `onCheck`, `onAdd` |
| `LabelChips` | view, editing, suggestions | `labels`, `suggestions`, `onChange` |
| `FolderSelect` | closed, open, creating | `folders`, `value`, `onChange`, `onCreate` |
| `LifetimeField` | sliding, fixed, keep; invalid | `value`, `allowedModes`, `maxIdle` |
| `UsageMeter` | normal, near, full, unlimited | `label`, `used`, `limit`, `atLimit` |
| `CloneDialog` | editing, cloning, error | `endpoint`, `options`, `onClone` |
| `PasswordSetting` | unset, set, editing, saving, error | `isSet`, `setAt`, `onSet`, `onRemove` |
| `StatePage` kind `locked` | idle, checking, wrong, rate-limited | `endpointId`, `onUnlock` |
| `CidrChips` | empty, filled, invalid entry, self-outside | `entries`, `clientIp`, `onChange` |
| `MembersTable` | owner, member, pending row | `members`, `invitations`, `myRole`, handlers |
| `InviteDialog` | editing, sending, unknown user, error | `onInvite` |
| `CodeEditor` | chunk loading, read-only, editable, focused, tab-escape, diagnostics | `value`, `language`, `onChange`, `diagnostics`, `completions`, `label`, `singleLine`, `maxBytes` |
| `ScriptEditor` | as `CodeEditor`, plus language loading | `value`, `languageMeta`, `onChange`, `diagnostics` |
| `VariablePicker` | closed, open, filtered empty, loading | `catalogue`, `sampleCapture`, `onInsert` |
| `CapturePicker` | latest following, pinned, empty endpoint, loading | `endpointId`, `kinds`, `value`, `onChange`, `onSendSample` |
| `TemplateEditor` | literal, template, invalid, read-only | `response`, `catalogue`, `onChange` |
| `TemplatePreview` | idle, rendering, ok, stale, error, no capture | `draft`, `captureId` |
| `RuleList` | empty, populated, reordering, saving order, error | `rules`, `activeId`, `onReorder`, `onToggle` |
| `RuleCard` | on, off, selected, dragging, recent failures | `rule`, `position`, `total` |
| `RuleEditor` | clean, dirty, saving, invalid, conflict, read-only | `rule`, `catalogue`, `onSave` |
| `ConditionGroup` | all, any, empty | `mode`, `conditions`, `onChange` |
| `ConditionRow` | complete, incomplete, invalid | `condition`, `catalogue`, `onChange`, `onRemove` |
| `StepList` | empty, populated | `steps`, `onChange` |
| `StepCard` | collapsed, expanded, invalid, dragging, highlighted, guarded | `step`, `index`, `schema`, `errors` |
| `AddStepMenu` | closed, open, filtered | `catalogue`, `onAdd` |
| `SchemaField` | per field kind; error; read-only | `field`, `value`, `error`, `onChange` |
| `TestRunner` | idle, running, done, stale, invalid draft, no capture, error, recorded run | `draft`, `captureId`, `autoRun`, `recordedRunId` |
| `TraceTimeline` | collapsed breadcrumb (medium), full | `trace`, `cursor`, `selected`, `onSelect` |
| `TraceEntry` | ran, failed, simulated, skipped, not reached, not matched | `entry` |
| `VariablesInspector` | empty, populated, filtered | `vars`, `previous`, `onInsert` |
| `OutgoingView` | request, email, notification | `item` |
| `RunLogTable` | loading, empty, populated, failed only | `runs`, `onOpen` |
| `KindBadge` | `MAIL`, `DNS` | `kind` |
| `EmailSummary` | default, no subject, auth results | `message` |
| `EmailHtmlFrame` | loading, ready, images blocked, images loaded, failed; width preset | `src`, `width`, `onLoadImages` |
| `AttachmentList` | empty, items, previewing | `attachments` |
| `DnsSummary` | answered, unanswered | `query` |
| `InboxScopeSelect` | all, folder, label | `scopes`, `value` |
| `EndpointChip` | default | `label`, `endpointId` |
| `SelectionBar` | hidden, n selected | `count`, `onExport`, `onDelete`, `onClear` |
| `ExportDialog` | configuring, running, done, failed, cancelled | `scopeCounts`, `formats`, `job` |
| `ImportDialog` | empty, parsing, preview, invalid, importing, done | `endpointId`, `onDone` |
| `RequestEditor` | default, invalid URL, refused | `value`, `onChange`, `errors` |
| `ResponseView` | idle, sending, ok, error, refused | `result` |
| `ScheduleList` / `ScheduleEditor` | active, paused; dirty, saving | `schedules`, `schedule` |
| `CadenceField` | interval, daily, expression; invalid; next runs | `value`, `nextRuns`, `onChange` |
| `ChannelTable` / `ChannelEditor` | ok, failing, testing | `channels`, `kinds` |
| `TriggerTable` | default, dirty | `triggers`, `channels` |
| `TokenTable` | empty, populated | `tokens`, `onRevoke` |
| `CreateTokenDialog` | editing, creating, error | `scopes`, `endpoints`, `expiryOptions` |
| `SecretReveal` | masked, shown, copied, close confirm | `secret` |
| `ApiReference` | loading, ready, error | `document`, `operationId` |
| `CliInstructions` | default, token created | `endpoint`, `cli` |
| `AccountMenu` | anonymous, signed in | `user`, `signInAvailable` |
| `FilterBuilder` | popover, sheet; dirty; count loading | `filters`, `onApply` |
| `SavedFiltersMenu` | empty, list, renaming | `saved`, `onApply`, `onSave` |

## 25. Token additions

Only two badge pairs and one surface are new; everything else reuses section 7.

| Role | Light text / bg | Dark text / bg | Contrast L / D |
|---|---|---|---|
| `kind-mail` (`MAIL` badge) | `#9A2F63` / `#FBE5EF` | `#FF9CC8` / `#3A1528` | 5.9 / 8.2 |
| `kind-dns` (`DNS` badge) | `#52601A` / `#EEF1DA` | `#C3D36B` / `#262C0E` | 6.0 / 8.9 |
| `frame-canvas` (email HTML frame) | `#FFFFFF` | `#FFFFFF` | Email HTML assumes white; 1 px `border-strong` around it |

| New meaning | Existing token |
|---|---|
| Template token in editors | `accent-text` on `accent-soft` (5.6 / 9.5) |
| Trace outcomes | Section 17.4 table |
| Variable new / changed | `success` / `warning` text markers `+` / `~` |
| Usage meter normal / near / full | `accent` / `warning` / `danger` |
| Card highlighted by tester | `accent-soft` fill |
| Invalid card | 3 px `danger` left bar plus error count text |
| Secret field | `surface-sunken`, `mono-data` |

## 26. Accessibility rules for the new editors

| Area | Rule |
|---|---|
| Code editors | Each has a visible label tied by `aria-labelledby`; Tab indents; `Esc` then `Tab` leaves the editor (CodeMirror tab-focus mode), stated in a 13 px hint under every editor; read-only editors do not trap Tab at all |
| Diagnostics | Never only colour or underline: an `ERRORS n` list outside the editor with `[Go]` buttons; the count is announced politely after validation settles, not on each keystroke |
| Completion and picker | CodeMirror completion keeps its ARIA combobox; `VariablePicker` is a Radix Popover with a listbox, typeahead filter, `Enter` inserts, focus returns to the field |
| Rule and step order | Drag is optional: every move is also a button and `Alt+Shift+Up/Down`; each move is announced with the new position |
| Cards | Each step is a `region` named "Step 2 of 3: Respond", with a heading button carrying `aria-expanded`; errors are linked by `aria-describedby` |
| Conditions | Each row is a `fieldset` with legend "Condition 1"; remove buttons are named "Remove condition 1" |
| Trace | An ordered list; the selected entry has `aria-current="step"`; manual Run announces once "Test finished: matched, 3 steps, 1 simulated"; stepping announces the entry text; auto-runs are silent |
| Variables inspector | Nested lists with disclosure buttons (no tree grid); copy and insert are buttons with names |
| Switches | `role="switch"` with visible On/Off text |
| Email frame | `title="Email HTML, scripts and links disabled"`; a skip link "Skip email content" before it; the Text section is the accessible alternative and comes first |
| Secret reveal | Focus moves to the secret field; its description says it is shown only once |
| Cadence | The expression field is followed by a plain-language summary and the next run times as text |
| Targets | 24 px minimum, 44 px for primary actions and drag handles on narrow |
| Motion | Reorder and drawer animations follow the reduced-motion rule of section 7 |

## 27. i18n key areas

Bundles follow the existing one-file-per-area layout in `web/src/i18n/en/`. Catalogue entries (step types,
fields, subjects, operators, channel kinds, scopes) have stable server ids; the interface owns their
translations in `catalogue.json` under keys built from those ids and lists every known key as a literal in
`i18n/catalogueKeys.ts` so the missing-key guard still greps them. Unknown ids fall back to the server's
English label rendered with `lang="en"`.

| Area | Bundle | Examples |
|---|---|---|
| `workspace` | `workspace.json` | `wl.workspace.section.access`, `wl.workspace.saveBar.unsaved`, `wl.workspace.conflict.message` |
| `general` | `workspace.json` | `wl.general.lifetime.sliding`, `wl.general.lifetime.outcome`, `wl.general.clone.title` |
| `access` | `access.json` | `wl.access.password.title`, `wl.access.allowList.outside`, `wl.access.members.role.editor` |
| `template` | `template.json` | `wl.template.useVariables`, `wl.template.preview.stale`, `wl.template.errors.count_one` |
| `variables` | `template.json` | `wl.variables.group.request`, `wl.variables.filter.placeholder` |
| `rules` | `rules.json` | `wl.rules.list.orderHint`, `wl.rules.onMatch.stop`, `wl.rules.moved` |
| `tester` | `rules.json` | `wl.tester.run`, `wl.tester.outcome.simulated`, `wl.tester.finished_other` |
| `runs` | `rules.json` | `wl.runs.filter.failedOnly`, `wl.runs.recorded.banner` |
| `catalogue` | `catalogue.json` | `wl.catalogue.step.respond.title`, `wl.catalogue.operator.contains`, `wl.catalogue.scope.capturesRead` |
| `mail` | `mail.json` | `wl.mail.section.html`, `wl.mail.images.load`, `wl.mail.images.warning` |
| `dns` | `mail.json` | `wl.dns.field.type`, `wl.dns.answer.none` |
| `addresses` | `workspace.json` | `wl.addresses.email.alsoCaught`, `wl.addresses.alias.taken` |
| `inbox` | `inbox.json` | `wl.inbox.scope.all`, `wl.inbox.empty.title` |
| `organize` | `inbox.json` | `wl.organize.folder.unfiled`, `wl.organize.source.local` |
| `composer` | `composer.json` | `wl.composer.send`, `wl.composer.replay.banner`, `wl.composer.notSent` |
| `schedules` | `composer.json` | `wl.schedules.cadence.interval`, `wl.schedules.nextRuns` |
| `bulk` | `bulk.json` | `wl.bulk.export.format.zip`, `wl.bulk.import.mode.replay`, `wl.bulk.export.progress` |
| `notify` | `account.json` | `wl.notify.trigger.ruleFails`, `wl.notify.channel.test` |
| `usage` | `account.json` | `wl.usage.captures.atLimit`, `wl.usage.footer` |
| `tokens` | `account.json` | `wl.tokens.secret.once`, `wl.tokens.revoke.confirm` |
| `account` | `account.json` | `wl.account.signOut`, `wl.account.menu.tokens` |
| `apireference` | `apireference.json` | `wl.apireference.operation.openInComposer`, `wl.apireference.download` |
| `cli` | `account.json` | `wl.cli.step.install`, `wl.cli.token.create` |
| `prefs` | `account.json` | `wl.prefs.editor.indent`, `wl.prefs.data.clearLocal` |
| `filters` | `filters.json` | `wl.filters.saved.save`, `wl.filters.in.body`, `wl.filters.range.custom` |
| `editor` | `common.json` | `wl.editor.tabHint`, `wl.editor.errors.go` |
| `shortcuts` | `common.json` | `wl.shortcuts.selectRow`, `wl.shortcuts.replay` |

## 28. Screen data requirements additions

What the interface needs, by capability; wire shapes belong to the API document.

| Capability | Needs |
|---|---|
| Instance | Flags per capability below; limits for each meter; the viewer's own address; whether sign-in is available to anonymous visitors; email domain and DNS zone |
| Templating | Variable catalogue for a capture (path, type, sample value, insertion text, group, description) and template delimiters; render preview for a draft response plus capture id (status, headers, body, diagnostics with line, column, length, severity); the response actually sent per capture |
| Rules | CRUD with a version for conflicts (`If-Match` planned, api.md section 15); reorder; catalogues of triggers, subjects with parameter kind, operators per value type, step types with field schemas, phase and summary template, script language meta (keywords, functions with signatures and docs, limits); validation with field paths |
| Test run | Dry run of a draft rule or of all rules with a draft substituted, against a capture: conditions with evaluated values, ordered trace entries (outcome, duration, variable deltas, output, logs, error position), simulated outputs, final response; real run on demand |
| Run log | Runs per capture and per rule, same trace shape as test runs, paged; a live event when a run finishes |
| Email | Kind on summaries (subject, from, size, attachment count); detail with headers, text part, HTML presence, attachments (id, name, type, size, content id, inline); HTML route with the policy of section 18.2 and a remote-images flag; attachment and source download routes |
| DNS | Kind on summaries (name, type, asker, transport); detail with flags, EDNS, question, answer; raw packet |
| Endpoint management | Labels, folder, aliases with availability check, lifetime modes allowed, allow list, viewer password set/remove and unlock, members with pending invitations, clone with options, a locked error code distinct from gone |
| Inbox | Captures across an endpoint set with endpoint id per item; one stream for the set with per-endpoint `seq` |
| Composer | Send a request through the server: status, headers, body prefix, timings, final URL, error codes as forwarding |
| Schedules | CRUD, pause, next run times for a cadence, history with request and response |
| Export, import | Export jobs (scope by ids, filter or all; format; progress; cancel; download link with expiry); bulk delete by ids; import upload, preview counts, mode, result |
| Notifications | Channel kinds with field schemas; channels CRUD; test send result; endpoint triggers; throttle |
| Usage | Counters with limits and the at-limit behaviour per meter, per endpoint and per account |
| Tokens | CRUD, scope catalogue, endpoint restriction, expiry options, secret in the create answer only, last used |
| API reference | The OpenAPI document at a stable URL |
| CLI | Install commands per platform and a command template |
| Search | Text scopes, header conditions, rule outcome filter, absolute ranges; saved filters CRUD |
| Interface headers | `frame-src 'self'` in the UI policy |

## 29. Frontend engineering additions

| Concern | Choice | Reason | Rejected |
|---|---|---|---|
| Editable editor | CodeMirror 6 editable build: `@codemirror/commands`, `@codemirror/autocomplete`, `@codemirror/lint` in a lazy `editor` chunk shared by templates, scripts, composer bodies | Already the viewer; one engine for all editing | Monaco: several MB, worker setup, CSP friction |
| Reorder | `@dnd-kit/core` and `@dnd-kit/sortable` (MIT), lazy with the rules chunk | Pointer, touch and keyboard sensors; no DOM cloning | Native HTML drag: no touch, poor keyboard story |
| Primitives | Add `@radix-ui/react-dialog`, `react-popover`, `react-switch`, `react-tabs` as section 10 planned | Focus management | |
| Forms | Controlled state per section with a draft object and a pure validator for client-side shape checks; the server is the source of semantic errors | Small, testable | Form libraries: one more abstraction for a dozen forms |
| Catalogues | Fetched once per session with long `staleTime`, typed in `api/types.ts` | Rarely change | |
| Folders | `features/` gains `editor`, `templating`, `rules`, `tester`, `mail`, `dns`, `inbox`, `organize`, `access`, `composer`, `schedules`, `bulk`, `notify`, `usage`, `tokens`, `apireference`, `account`, `filters`; `routes/` gains `workspace`, `inbox`, `compose` (served at `/send`), `account`, `docs` | Matches section 10 | |

| Budget | Target |
|---|---|
| `editor` chunk, gzip | <= 120 KB on top of the viewer chunk |
| Rules route chunk without editor, gzip | <= 60 KB |
| API reference chunk, gzip | <= 40 KB |
| Template preview round trip on a local network | <= 300 ms after the debounce |

| Key | Action | Scope |
|---|---|---|
| `x` | Toggle selection of the focused row (enters select mode) | Endpoint view, inbox |
| `Shift+E` | Export dialog | Endpoint view, inbox |
| `Shift+R` | Replay selected capture in the composer | Capture selected |
| `i` | Open inbox | Anywhere |
| `Mod+Enter` | Run test, render preview, send request | Rule editor, Response, composer (works inside editors) |
| `Mod+S` | Save the current section | Workspace (works inside editors) |
| `Alt+Shift+Up` / `Down` | Move focused rule or step | Rules |
| `[` / `]` | Previous / next trace entry | Test runner |
| `Esc`, then `Tab` | Leave a code editor | Editors |

## 30. Milestones F15 and up

Risky marks follow the working rules: `untrusted` (rendering foreign content), `editor` (code editors),
`async` (stream and ordering concurrency), `security` (secrets, access). Every milestone can start against
MSW handlers and fixtures written from this document and the API document; "Real backend" says what its done
criterion additionally needs. F10 and F11 are now built inside F16's workspace.

| # | Scope | Backend capability | Risky | Done when |
|---|---|---|---|---|
| F15 | Editor kit: `CodeEditor` (editable, lazy chunk, single-line variant, tab-escape, lint bridge for server diagnostics, completion source interface), `VariablePicker`, `CapturePicker` | None | editor | Token sample page hosts all three with mocked catalogue and diagnostics; keyboard can enter and leave every editor; chunk within budget |
| F16 | Workspace shell: `/e/:id/settings/:section` routes, section nav in three layouts, `SaveBar`, dirty guard, `ConflictBanner`, forwarding redirect, rail auto-collapse | None | - | Playwright opens every section at 360, 1024, 1440; dirty guard blocks leaving; redirect works |
| F17 | Response templating: `Use variables`, template body and header values, picker, server preview with capture choice and pin, errors and warnings, size counter | Templating | - | MSW: an error in the body underlines, lists and disables Save; preview follows Latest. Real backend: a saved template answers a curl with the rendered body |
| F18 | Rule list: cards, create blank, script, duplicate, delete, enable switch, reorder by drag and keyboard with announcements | Rules CRUD and order | - | Order survives reload; keyboard reorder announced; failed save reverts with a toast |
| F19 | Rule editor, conditions: trigger kinds, `ConditionGroup`, subject, operator and value widgets from the catalogue, on match, save, discard, conflict, field errors on rows | Rules CRUD, condition catalogue | - | A server field error lands on its row and focuses it; conflict banner on a simulated concurrent change |
| F20 | Rule steps: `StepList`, insertion points, `AddStepMenu`, `SchemaField` kinds, guards, collapse and expand, step moves, "Produces" line, phase badge | Step catalogue | - | Every field kind renders from a fixture schema; a step added between two others saves in that position |
| F21 | Script step: `ScriptEditor` with language highlighting, function and variable completion, hover docs, server diagnostics, full-height script rule, limits line | Scripting language meta and validation | editor | A syntax error from the server marks its line; completion lists catalogue functions |
| F22 | Test runner: dry run of the draft against a capture, conditions entry, trace, stepping, variables inspector, simulated outputs, auto-run with abort and ordering, stale marker, Run for real, Test all rules | Dry run with trace | async | Unit test: out-of-order responses never replace a newer run; Playwright: stepping highlights cards; Test all shows a stop |
| F23 | Run logs: capture sections `Response sent` and `Rules`, rule Runs tab, live run events, open a recorded run in the tester | Run log, live run event, stored sent response | - | A real run appears live in both places and opens read-only in the tester |
| F24 | Addresses popover and empty-state samples, `KindBadge`, row variants per kind, kind filter, DNS detail | Email and DNS capture kinds | - | Fixtures of three kinds render rows and DNS detail as in section 18 at three widths |
| F25 | Email detail without HTML: summary, text, headers, attachments with download and raster preview, raw source | Email capture detail | untrusted | Fixtures with hostile names, headers and MIME types render as text only; downloads use attachment routes |
| F26 | Email HTML frame: sandboxed iframe from the backend route, remote images opt-in, width presets, full view route | Email HTML route with its policy; `frame-src 'self'` | untrusted | Real backend: a message with script, form, meta refresh and remote image runs nothing, loads nothing until opted in, cannot reach the parent (Playwright) |
| F27 | General additions: folder, labels, aliases with availability, lifetime modes, usage meters, clone dialog | Endpoint management | - | Clone creates an endpoint with the chosen parts; lifetime text matches the chosen mode |
| F28 | Access: viewer password and `locked` unlock page, sender allow list, members and invitations (replaces F10's sso sharing) | Access controls, members | security | Wrong password shows the error without leaking existence; allow list rejects invalid ranges; viewer role sees read-only |
| F29 | Rail folders and labels, merged My endpoints page, move to account | Endpoint management, accounts | - | Local and account endpoints appear once each with source; move to account keeps label and folder |
| F30 | Combined inbox: route, scope select, multiplexed stream, endpoint chips, per-row actions by role | Combined list and stream | async | Arrivals from two endpoints merge without duplicates after a forced reconnect |
| F31 | Filter builder, extended URL parameters, saved filters | Search scopes, saved filters | - | A saved filter reopens the same result from its URL; header condition narrows the list |
| F32 | Select mode, bulk delete, export dialog with job progress, cancel, download, recent exports | Export jobs, bulk delete | - | Export of 48 filtered captures as ZIP downloads with the reported size; cancel stops the job |
| F33 | Import dialog: upload, preview, modes, result | Import | - | A previously exported JSON imports into a new endpoint with original times |
| F34 | Composer: `RequestEditor`, send through the server, `ResponseView`, history, replay prefill, URL errors | Outgoing requests | - | Replay of a fixture capture sends the same body; a private target shows the refusal on the field |
| F35 | Schedules: list, editor with `RequestEditor`, `CadenceField` with next runs, pause, history | Scheduled requests | - | A 1-minute schedule produces history entries; pause stops them |
| F36 | API tokens: table, create dialog with scope grid, endpoint restriction, expiry, `SecretReveal`, revoke | Personal tokens | security | Secret is absent from query cache, storage and URL after closing (unit test); revoked token answers 401 (real backend) |
| F37 | API reference: own renderer, operation deep links, samples, Open in composer | Served OpenAPI document | - | Every operation of the served document renders; chunk within budget |
| F38 | Local forwarding section: install tabs, command template, port field, inline token | CLI distribution info; tokens for the token part | - | Copied command runs with the shipped CLI against the endpoint |
| F39 | Notifications: channel table and editor from schemas, test send, endpoint triggers, throttle, browser notifications, Notify step field | Notifications | - | Test send shows success and failure inline; a trigger fires on a capture |
| F40 | Usage and limits page, meters, at-limit texts, header link | Usage counters | - | Each meter shows the at-limit sentence; 90 % and 100 % states render |
| F41 | Account menu, `/account`, sign out, `/settings` preferences groups, new shortcuts in the dialog | Accounts for profile; none for preferences | - | Preferences persist and apply without reload; sign out returns home |
| F42 | Parity hardening: screenshots of sections 15 to 23 at 360, 1024, 1440 in both themes, axe on editors and dialogs, new chunk budgets, end-to-end "create rule, test, receive, see run" | All of the above | - | CI enforces budgets; axe reports no serious issues |

Status 2026-11-22 (shipped parts that differ from the rows above):

| # | Shipped | Deferred |
|---|---|---|
| F19, F19b | Rule editor page (`RuleEditorPage`): Build with conditions and steps, save, discard, dirty guard, field errors, conflict check by re-reading the list, full-height script rules, `/rules/new` | Trigger kinds, on match, step guards (not in v1, section 17.1) |
| F20 | Step list, add menu, schema fields, moves, phase badge | Guards |
| F22 | Tester inside the editor and Test all rules, trace entries, stepping, ordering, Run for real by replay | Conditions entry, variables inspector, final response (need dry-run fields), recorded runs |
| F23 | Capture sections, live run events, Runs tab per endpoint, Open in tester | Per-rule filter, "n failed today", recorded run view |
| F32 | Select mode, export dialog with direct download, bulk delete one by one (at most 500) | Export jobs, progress, recent exports (exports stream directly, architecture section 27); a bulk delete route |
| F37 | Section 22.3 with the shipped notes there | – |
| F39 | Section 22.5 "Shipped (F39) and deferred" | Triggers, throttles, account-wide channels, push removal |

| Group | Milestones |
|---|---|
| Can start now, no new backend | F15, F16, F41 (preferences part), F37 (against a fixture document) |
| Built on MSW first, finished when the capability lands | F17 to F25, F27, F29, F31 to F36, F38 to F40 |
| Need the real backend to be done | F26 (policy headers), F30 (stream), F42 |

Recommended order: finish F5, then F6 and F9 (live and filters underlie the inbox and saved filters);
F15 and F16; F10 and F11 inside the workspace; then the rules track F17, F18, F19, F20, F22, F21, F23
(templating first because its picker, preview and diagnostics are reused by every rule step); in parallel,
low-risk forms for the standard coder: F27, F34, F35, F40, F41, F37, F38; email and DNS F24, F25, F26 as
soon as those capture kinds exist; then F28, F29, F30, F31, F32, F33, F36, F39; F42 last. Risky milestones
for the stronger coder: F15, F21 (editor), F22, F30 (async), F25, F26 (untrusted), F28, F36 (security).

## 31. Open questions for the owner (parity)

1. Remote images in email HTML: offer the opt-in `[Load]` button (reveals that the message was opened), or
   never load remote images at all?
2. `Run for real` in the rule tester performs real side effects from the editor for editors and owners. Keep
   it, or allow side effects only through real captures?
3. Accounts on the public instance: tokens, account-owned endpoints, saved filters on the server and channel
   lists assume a signed-in user. If the public instance stays anonymous, those areas are hidden there and
   the inbox covers only this browser's endpoints.
