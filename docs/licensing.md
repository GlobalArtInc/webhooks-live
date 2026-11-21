# Licensing and provenance

Status: draft 1, 2026-11-21.

## Where webhooks.live comes from

webhooks.live replaces the open-source PHP/Angular.js request catcher that GlobalArt runs at
`webhooks.globalart.hu` from the private mirror `GlobalArtInc/webhooks`.

| Fact | Value |
|---|---|
| Upstream licence | MIT, Copyright (c) 2017 Simon Fredsted |
| Obligation under MIT | Keep the copyright and licence notice in copies of the upstream code |
| Obligation that actually arose | None beyond that: the mirror kept the notice, and running the code is permitted |

The rewrite is **not** a response to a licence violation. Its reasons are maintainability (an end-of-life
PHP runtime and framework, a separate Node process for live updates, a single-replica limit), a stack the
team can own end to end (Rust and React), and a clean ownership position: webhooks.live contains none of the
upstream code, so the team alone decides how it is licensed and distributed.

## Clean-rewrite rules

The upstream behaviour was studied to write [product-spec.md](product-spec.md). The implementation follows
these rules so that nothing of the upstream expression is carried over:

1. No source file, function body, template, stylesheet, image, icon, or string is copied or translated from
   the upstream repository. Behaviour is re-specified in [product-spec.md](product-spec.md) and implemented
   from that document.
2. Names, API paths, JSON field names, data model, and visual design are webhooks.live's own and differ from
   upstream.
3. The upstream product name and branding are not used. The name `Webhook.site` belongs to a commercial
   service; webhooks.live never presents itself as, or as compatible with, that service.
4. Nothing from the upstream repository is vendored, and its git history is not part of the webhooks.live
   repository. The webhooks.live repository starts empty.
5. Third-party libraries are used only through their published packages and under their own licences.

Functionality (receive any HTTP request and show it live) is not protected by copyright. Expression is,
and rules 1, 2, and 4 keep webhooks.live clear of it.

## Licence of webhooks.live itself

Undecided; this is the owner's call. Until it is made the repository is private and carries no `LICENSE`
file, which means all rights are reserved.

| Option | Consequence |
|---|---|
| Proprietary, private repository | Full control; nobody outside GlobalArt may use or contribute |
| Permissive (Apache-2.0 or MIT) | Can be published and self-hosted by others; no control over forks |
| Source-available (for example BUSL or an equivalent) | Public code with usage limits; needs a lawyer's pass |

## Dependency licence policy

| Allowed | Denied |
|---|---|
| MIT, Apache-2.0, BSD-2/3-Clause, ISC, Unicode, Zlib, MPL-2.0 (file-level copyleft only) | GPL, AGPL, LGPL for statically linked code, SSPL, unlicensed, and anything with a non-commercial clause |

- Font files may additionally be under OFL-1.1 (the SIL Open Font License), because it permits embedding
  and redistribution and only restricts selling the font on its own. Code dependencies may not.
- Rust: `cargo-deny` with this policy in `deny.toml`, run in CI on every push.
- Frontend: a licence check over the production dependency tree in CI, same policy.
- A generated third-party notices file ships inside the container image and the embedded UI.
