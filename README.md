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

## Repository layout

```
crates/       Rust workspace
web/          React interface
docs/         design documents
tools/        helper scripts
Containerfile container image build
```

The Helm chart lives in the GlobalArt helm-charts repository. The layout is described in [docs/architecture.md](docs/architecture.md).
