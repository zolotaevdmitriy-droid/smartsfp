#!/usr/bin/env bash
# ACM-UZ developer wrapper. All builds and tests run inside the docker
# builder image — host needs only Docker + bash (Git Bash on Windows works).
#
#   ./dev.sh build-image      First-time setup (downloads ~2-3 GB).
#   ./dev.sh shell            Interactive shell in builder.
#   ./dev.sh build            Cross-compile cryptod/agent/controller to dist/.
#   ./dev.sh fmt              cargo fmt + gofmt.
#   ./dev.sh test             Unit tests.
#   ./dev.sh proto            Regenerate Go bindings from proto/.
#   ./dev.sh clean            Clear build artifacts.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE_TAG="acm-uz/builder:latest"

# Named docker volumes for cargo registry / go module cache so we don't
# re-download every time. Survive image rebuilds.
CARGO_VOL="acm-uz-cargo-registry"
GO_VOL="acm-uz-go-modcache"

# On Windows / Git Bash MSYS auto-converts /work to C:\work which we DON'T
# want — keep paths literal. MSYS_NO_PATHCONV=1 disables that conversion.
export MSYS_NO_PATHCONV=1

run_in_container() {
  docker run --rm -i \
    -v "${REPO_ROOT}":/work \
    -v "${CARGO_VOL}":/usr/local/cargo/registry \
    -v "${GO_VOL}":/go/pkg/mod \
    -w /work \
    "${IMAGE_TAG}" \
    bash -c "$1"
}

run_in_container_tty() {
  docker run --rm -it \
    -v "${REPO_ROOT}":/work \
    -v "${CARGO_VOL}":/usr/local/cargo/registry \
    -v "${GO_VOL}":/go/pkg/mod \
    -w /work \
    "${IMAGE_TAG}" \
    "$@"
}

cmd="${1:-help}"
shift || true

case "$cmd" in
  build-image)
    echo ">>> Building builder image (this may take a while on first run)"
    docker build -t "${IMAGE_TAG}" -f docker/builder.Dockerfile docker/
    ;;

  shell)
    run_in_container_tty bash
    ;;

  build)
    mkdir -p dist
    run_in_container '
      set -e
      echo "=== Build cryptod (aarch64) ==="
      cd cryptod
      cargo build --release --target aarch64-unknown-linux-gnu
      cd ..

      echo "=== Build agent (aarch64) ==="
      cd agent
      GOOS=linux GOARCH=arm64 CGO_ENABLED=0 go build -ldflags="-s -w" \
          -o ../dist/acm-agent ./cmd/acm-agent
      GOOS=linux GOARCH=arm64 CGO_ENABLED=0 go build -ldflags="-s -w" \
          -o ../dist/acm-cli ./cmd/acm-cli
      GOOS=linux GOARCH=arm64 CGO_ENABLED=0 go build -ldflags="-s -w" \
          -o ../dist/acm-encdec-test ./cmd/acm-encdec-test
      cd ..

      echo "=== Build controller (amd64) ==="
      cd controller
      GOOS=linux GOARCH=amd64 CGO_ENABLED=0 go build -ldflags="-s -w" \
          -o ../dist/acm-controller ./cmd/acm-controller
      cd ..

      echo "=== Copy cryptod binary ==="
      cp cryptod/target/aarch64-unknown-linux-gnu/release/acm-cryptod dist/

      echo "=== Artifacts ==="
      ls -la dist/
      file dist/*
    '
    ;;

  fmt)
    run_in_container '
      set -e
      cd cryptod && cargo fmt --all && cd ..
      cd agent && gofmt -w . && cd ..
      cd controller && gofmt -w . && cd ..
      echo "fmt OK"
    '
    ;;

  test)
    run_in_container '
      set -e
      cd cryptod && cargo test --workspace && cd ..
      cd agent && go test ./... && cd ..
      cd controller && go test ./... && cd ..
    '
    ;;

  proto)
    run_in_container '
      set -e
      mkdir -p proto/gen/go
      protoc --proto_path=proto \
             --go_out=proto/gen/go \
             --go_opt=paths=source_relative \
             --go-grpc_out=proto/gen/go \
             --go-grpc_opt=paths=source_relative \
             proto/*.proto
      echo "Generated Go bindings:"
      ls -la proto/gen/go/
    '
    ;;

  clean)
    rm -rf dist
    run_in_container '
      cd cryptod && cargo clean || true
      cd ../agent && go clean -cache -testcache || true
      cd ../controller && go clean -cache -testcache || true
    ' || true
    ;;

  help|*)
    cat <<EOF
Usage: ./dev.sh <command>

  build-image   Build docker builder (one-time, ~2-3 GB download).
  shell         Interactive shell inside the builder.
  build         Cross-compile everything to dist/.
  fmt           cargo fmt + gofmt.
  test          Run unit tests.
  proto         Regenerate Go bindings from proto/.
  clean         Clear build artifacts.

Artifacts (after \`./dev.sh build\`):
  dist/acm-cryptod      aarch64  -- to ISM4120I
  dist/acm-agent        aarch64  -- to ISM4120I
  dist/acm-cli          aarch64  -- to ISM4120I
  dist/acm-controller   amd64    -- to management server
EOF
    ;;
esac
