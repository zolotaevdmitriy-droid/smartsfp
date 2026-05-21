# ACM-UZ build image.
# All cross-compilation for the Smart SFP ISM4120I (aarch64) and management
# server (amd64) happens inside this container.
#
# Build:   docker build -t acm-uz/builder:latest -f docker/builder.Dockerfile docker/
# Or:      ./dev.sh build-image
#
# Inside container the repo is mounted at /work.

FROM debian:12-slim

ENV DEBIAN_FRONTEND=noninteractive
ENV LANG=C.UTF-8 LC_ALL=C.UTF-8

# Base utilities + native and cross toolchain.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl wget gnupg lsb-release \
        git make cmake ninja-build pkg-config build-essential \
        gcc-aarch64-linux-gnu binutils-aarch64-linux-gnu \
        libc6-dev-arm64-cross linux-libc-dev-arm64-cross \
        libssl-dev libclang-dev \
        unzip xz-utils file vim less \
    && rm -rf /var/lib/apt/lists/*

# -- Rust (stable + aarch64 cross target) -----------------------------------
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo
ENV PATH=$CARGO_HOME/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
        sh -s -- -y --default-toolchain stable --no-modify-path --profile minimal \
    && rustup target add aarch64-unknown-linux-gnu \
    && rustup component add rustfmt clippy

# Cross-link configuration: cargo uses aarch64-linux-gnu-gcc as linker for
# the aarch64 target. Equivalent of placing this in .cargo/config.toml but
# baked into the image as defaults.
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
ENV CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc

# -- Go ----------------------------------------------------------------------
ARG GO_VERSION=1.23.4
RUN ARCH=$(dpkg --print-architecture) \
    && case "$ARCH" in \
         amd64) GOARCH=amd64 ;; \
         arm64) GOARCH=arm64 ;; \
         *) echo "Unsupported arch: $ARCH" && exit 1 ;; \
       esac \
    && wget -q -O /tmp/go.tar.gz "https://go.dev/dl/go${GO_VERSION}.linux-${GOARCH}.tar.gz" \
    && tar -C /usr/local -xzf /tmp/go.tar.gz \
    && rm /tmp/go.tar.gz
ENV PATH=/usr/local/go/bin:$PATH
ENV GOPATH=/go GOMODCACHE=/go/pkg/mod GOCACHE=/go/cache
RUN mkdir -p /go/pkg/mod /go/cache

# -- Node.js (for Svelte UI builds) -----------------------------------------
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

# -- protoc + Go plugins -----------------------------------------------------
# (Rust uses prost-build crate which links its own protoc — but having
#  the binary system-wide is convenient for code-gen scripts.)
ARG PROTOC_VERSION=28.3
RUN ARCH=$(uname -m) \
    && case "$ARCH" in \
         x86_64)  PARCH=x86_64 ;; \
         aarch64) PARCH=aarch_64 ;; \
         *) echo "Unsupported arch: $ARCH" && exit 1 ;; \
       esac \
    && wget -q -O /tmp/protoc.zip \
        "https://github.com/protocolbuffers/protobuf/releases/download/v${PROTOC_VERSION}/protoc-${PROTOC_VERSION}-linux-${PARCH}.zip" \
    && unzip -q /tmp/protoc.zip -d /usr/local \
    && chmod +x /usr/local/bin/protoc \
    && rm /tmp/protoc.zip

RUN GOBIN=/usr/local/bin go install google.golang.org/protobuf/cmd/protoc-gen-go@v1.34.2 \
    && GOBIN=/usr/local/bin go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@v1.5.1 \
    && go clean -cache -modcache

# -- Working directory -------------------------------------------------------
WORKDIR /work

# Sanity print on container start.
CMD ["bash", "-c", "echo 'ACM-UZ builder ready.'; echo '  rust    : ' $(rustc --version); echo '  cargo   : ' $(cargo --version); echo '  go      : ' $(go version); echo '  node    : ' $(node --version); echo '  protoc  : ' $(protoc --version); exec bash"]
