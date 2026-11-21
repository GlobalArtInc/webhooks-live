# webhooks.live

Self-hosted HTTP request catcher. Create an endpoint, point a webhook sender at it, and watch every
request arrive in real time with its full headers, query, and body. Forward captures to another URL,
compare two of them, copy any of them as `curl`.

Backend in Rust, interface in React. One binary, one container image.

Status: design phase. Nothing is implemented yet.

## Documents

| Document | What it answers |
|---|---|
| [docs/product-spec.md](docs/product-spec.md) | What webhooks.live does: capture, endpoints, viewing, forwarding, access modes, limits |
| [docs/architecture.md](docs/architecture.md) | How the backend is built: storage, live updates, crates, security, deployment, milestones |
| [docs/api.md](docs/api.md) | The HTTP contract between the backend and the interface |
| [docs/ui-design.md](docs/ui-design.md) | How the interface looks and behaves: screens, tokens, components, milestones |
| [docs/licensing.md](docs/licensing.md) | Where webhooks.live comes from, the clean-rewrite rules, dependency licence policy |

## Why it exists

GlobalArt runs an open-source PHP/Angular.js request catcher at `webhooks.globalart.hu`. It rests on an
end-of-life runtime, needs a separate Node process for live updates, and cannot run more than one replica.
webhooks.live is a from-scratch rewrite with its own design that the team can maintain and extend. No code is
carried over from the old project; see [docs/licensing.md](docs/licensing.md).

## Repository layout (planned)

```
crates/      Rust workspace
web/         React interface
docs/        design documents
deploy/      container image and Helm chart
```

The final layout is fixed in [docs/architecture.md](docs/architecture.md).
