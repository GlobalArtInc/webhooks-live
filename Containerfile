FROM node:24-bookworm-slim AS web

ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0 \
    CI=true

WORKDIR /src/web
COPY web/package.json web/pnpm-lock.yaml ./
RUN --mount=type=cache,target=/root/.cache/node/corepack,sharing=locked \
    --mount=type=cache,target=/root/.local/share/pnpm/store,sharing=locked \
    corepack pnpm install --frozen-lockfile
COPY web ./
RUN --mount=type=cache,target=/root/.cache/node/corepack,sharing=locked \
    corepack pnpm build

FROM rust:1-trixie AS build

ARG TARGETARCH

ENV CC_x86_64_unknown_linux_musl=musl-gcc \
    CC_aarch64_unknown_linux_musl=musl-gcc

RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY rust-toolchain.toml ./
RUN case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-musl ;; \
      arm64) triple=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && echo "$triple" > /opt/rust-target \
 && rustup show \
 && rustup target add "$triple"

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /src/web/dist ./web/dist

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo fetch --locked

RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    triple="$(cat /opt/rust-target)" \
 && cargo build --release --locked --target "$triple" --package whl-server \
 && install -D -m 0755 "target/$triple/release/webhooks-live" /out/webhooks-live

FROM gcr.io/distroless/static-debian13:nonroot

COPY --from=build /out/webhooks-live /usr/local/bin/webhooks-live

EXPOSE 8080 9090
ENTRYPOINT ["/usr/local/bin/webhooks-live"]
CMD ["serve"]
USER nonroot
