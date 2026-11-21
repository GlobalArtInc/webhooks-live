#!/usr/bin/env bash
set -euo pipefail

name=webhooks-live-test-db
image=docker.io/library/postgres:17
host=127.0.0.1
port=55440
password=webhooks-test-only
url="postgres://postgres:${password}@${host}:${port}/postgres"

usage() {
    echo "usage: $0 up|down|url" >&2
    exit 2
}

wait_ready() {
    local attempt=0
    until podman exec "$name" pg_isready -h 127.0.0.1 -p 5432 -U postgres >/dev/null 2>&1; do
        attempt=$((attempt + 1))
        if [ "$attempt" -ge 60 ]; then
            echo "postgres did not become ready within 60 seconds" >&2
            podman logs --tail 40 "$name" >&2 || true
            exit 1
        fi
        sleep 1
    done
}

case "${1:-}" in
up)
    if podman container exists "$name"; then
        podman start "$name" >/dev/null
    else
        podman run --detach --name "$name" \
            --env POSTGRES_PASSWORD="$password" \
            --publish "$host:$port:5432" \
            "$image" >/dev/null
    fi
    wait_ready
    echo "$name is ready"
    echo "WEBHOOKS_LIVE_TEST_DATABASE_URL=$url"
    ;;
down)
    if podman container exists "$name"; then
        podman rm --force "$name" >/dev/null
    fi
    echo "$name removed"
    ;;
url)
    echo "WEBHOOKS_LIVE_TEST_DATABASE_URL=$url"
    ;;
*)
    usage
    ;;
esac
