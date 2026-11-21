# webhooks.live interface design

Status: draft 1, 2026-11-21. Requirements: [product-spec.md](product-spec.md), sections 6 and 7 above all.
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
| All on one page | Settings and forwarding as side sheets over the live list, deep-linkable |

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
| `/e/:endpointId/settings` | Settings sheet over endpoint view | Narrow: full screen |
| `/e/:endpointId/forwarding` | Forwarding sheet over endpoint view | Narrow: full screen |
| `/e/:endpointId/diff/:a/:b` | Diff of two captures | Replaces the detail pane; narrow: full screen |
| `/endpoints` | My endpoints | Narrow only as a page; wide shows them in the rail |
| `/signin` | Sign-in (sso mode) | `?next=` returns to the original route |
| `?q=&method=&status=&since=&order=` | Filter state on endpoint routes | `order=oldest` flips V1 sort |

| State | Trigger | Shows |
|---|---|---|
| Empty endpoint | Endpoint exists, zero captures | Capture URL large, copyable sample `curl`, "Waiting for the first request" with a slow pulse, response config summary |
| Gone (410) | Endpoint unknown or expired | Full page: "This endpoint no longer exists", Create new, Remove from my endpoints; no list chrome |
| Not shared (403, sso) | Signed in, not the creator or a share | "Ask the owner to share it", signed-in identity, Sign out |
| Sign-in (sso) | No session on any non-capture route | Wordmark, one button "Sign in with {provider}", nothing else |
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
| Filters | Inline chip row | Popover | Bottom sheet |
| Expiry and capacity | Header text | `[...]` menu | `[...]` menu |
| Settings, forwarding | Right sheet 480 px | Right sheet 440 px | Full-screen route |
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
| curl | POSIX shell, every value single-quoted with `'\''` escaping, `--data-binary`; binary bodies are embedded as `printf '<octal escapes>' | curl ... --data-binary @-` (POSIX `\ooo` escapes, `%` written as `%%`), so the command stays self-contained |
| HAR | HAR 1.2 log with one entry built from the capture; response block from the answered status |
| JSON | Full capture as JSON |
| URL | Captured URL |
| Download JSON / Download raw body / Download HAR | Same content as files |

Exports are pure client-side functions over the full capture. Rejected: server-rendered exports, because
the client already holds the data and pure functions are trivially unit-tested for the quoting defect
listed in spec section 12.

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

Settings sheet: response status, content type, body (small code editor), delay slider 0-10 s, CORS toggle,
ring mode toggle when the server allows it, capacity meter, expiry ("sliding, 6 days of inactivity
left"), local label; sso mode adds "Shared with" (add/remove users); danger zone: Delete all captures,
Delete endpoint, each behind a confirm dialog.

Forwarding sheet: rule list with enabled toggle; rule editor with target URL, method override, content
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
| `SettingsSheet` | view, dirty, saving, error | `endpoint`, `serverLimits`, `mode` |
| `ForwardingSheet` / `ForwardRuleEditor` | list, empty, editing, field-error | `rules`, `onSave`, `onTest` |
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
| Catch-up | List the first page newer than a known capture id |
| Filters | Server-side filter by method set, override status class, time range, free text over headers and body; total count for the filtered set |
| Detail | Everything in C7, answered status, forward results (target, status, duration, error, time); raw body fetched separately |
| Header and settings | Capture URL, capture count, capacity, ring mode, expiry time and idle window, response config, CORS, delay |
| Server capabilities | Access mode, max captures, body limit, ring mode allowed, private forward targets allowed, provider name for sign-in |
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

Untrusted content rules: no `dangerouslySetInnerHTML`; captured HTML and SVG are text; header values are
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
| F10 | Settings sheet: response config, CORS, delay, ring mode, delete all, delete endpoint, sso sharing | Saved settings change what a test request receives |
| F11 | Forwarding sheet, rule editor, forward result panel, resend dialog | Forward result appears on a capture live; SSRF refusal shows on the field |
| F12 | Diff view, mark and compare, hide volatile | Two fixtures show header and JSON body differences side-by-side and unified |
| F13 | Shortcuts, shortcuts dialog, a11y pass | Every shortcut in the table has a Playwright case; axe reports no serious issues |
| F14 | Hardening: screenshot baselines at 360/1024/1440 in both themes, bundle-size check, 500-row arrival perf check | Budgets in section 10 enforced in CI |

F6 (asynchronous stream handling) and F7 (rendering untrusted bodies) fall in the risky areas of the
working rules and should go to the stronger coder.

## 12. Open questions for the owner

1. IBM Plex is licensed OFL-1.1, which the dependency policy in [licensing.md](licensing.md) does not
   list. Allow OFL for font assets, or use the system font stack and give up the family tie?
2. Is per-user sharing in sso mode (spec section 8) part of the first release? If not, F10 drops
   "Shared with" and the not-shared state stays a plain 403.
3. Launch languages: English and Russian only, or more targets in the `webhooks-live` OwnLate project?
