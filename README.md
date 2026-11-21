# webhooks.live

Self-hosted HTTP request catcher. Create an endpoint, point a webhook sender at it, and watch every
request arrive in real time with its full headers, query, and body. Forward captures to another URL,
compare two of them, copy any of them as `curl`.

Backend in Rust, interface in React. One binary, one container image.

Status: under development.

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

## Quick start

The image is `globalartltd/webhooks-live` on Docker Hub (linux/amd64). It needs a PostgreSQL database and two variables: `WEBHOOKS_LIVE_PUBLIC_URL` (the public URL that webhook senders use) and `WEBHOOKS_LIVE_DATABASE_URL` (the PostgreSQL connection string).

```sh
export WEBHOOKS_LIVE_PUBLIC_URL=https://hooks.example.com
export WEBHOOKS_LIVE_DATABASE_URL=postgres://user:password@db-host:5432/webhooks_live

docker run --rm -e WEBHOOKS_LIVE_PUBLIC_URL -e WEBHOOKS_LIVE_DATABASE_URL globalartltd/webhooks-live:latest migrate up
docker run -d -p 8080:8080 -p 9090:9090 -e WEBHOOKS_LIVE_PUBLIC_URL -e WEBHOOKS_LIVE_DATABASE_URL globalartltd/webhooks-live:latest
```

Run `migrate up` once before the first start and again after each upgrade. Port 8080 serves the interface and receives captured requests. Port 9090 is the admin listener (`/healthz`, `/readyz`, `/metrics`) and should not be exposed publicly.

The full configuration table is in [docs/architecture.md](docs/architecture.md), section 11.
