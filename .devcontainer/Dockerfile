# Development container with the toolchain CI uses: Rust with the
# wasm32v1-none target, rustfmt and clippy, and the stellar CLI.
#
# The versions match the reproducible-build Dockerfile at the repository root
# (Rust 1.100.0, stellar CLI 28.1.0 checked against its release sha256). Unlike
# that one, this image is not forced to linux/amd64, so it runs natively on
# arm64 hosts such as Apple silicon. To reproduce the deployed wasm bytes, use
# the root Dockerfile (docs/building/build-and-test.md), not this container.
FROM rust:1.100.0-slim-bookworm

ARG STELLAR_CLI_VERSION=28.1.0
ARG STELLAR_CLI_SHA256_AMD64=c1680deee94301d33ada7a17f98411e642a4248c727afbd2e43050d345746462
ARG STELLAR_CLI_SHA256_ARM64=b1e6ab53b0dd673400110fbd665a525cd0a969449943a2990a782f20cc7abe98

# rust-toolchain.toml asks for `stable`; this keeps the container on the
# pinned release instead of pulling whatever stable is today.
ENV RUSTUP_TOOLCHAIN=1.100.0

# git for the editor, curl to fetch the CLI; the CLI links libdbus and libudev.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl git libdbus-1-3 libudev1 \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32v1-none \
    && rustup component add rustfmt clippy

RUN case "$(dpkg --print-architecture)" in \
        amd64) triple=x86_64-unknown-linux-gnu; sum="$STELLAR_CLI_SHA256_AMD64" ;; \
        arm64) triple=aarch64-unknown-linux-gnu; sum="$STELLAR_CLI_SHA256_ARM64" ;; \
        *) echo "no stellar CLI release for $(dpkg --print-architecture)" >&2; exit 1 ;; \
    esac \
    && curl -sSfL -o /tmp/stellar.tar.gz \
        "https://github.com/stellar/stellar-cli/releases/download/v${STELLAR_CLI_VERSION}/stellar-cli-${STELLAR_CLI_VERSION}-${triple}.tar.gz" \
    && echo "${sum}  /tmp/stellar.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/stellar.tar.gz -C /usr/local/bin stellar \
    && rm /tmp/stellar.tar.gz \
    && stellar --version \
    && rustc --version
