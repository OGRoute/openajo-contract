# Reproducible wasm build for the `circle` and `reputation` contracts.
#
# Every input that affects the wasm bytes is pinned here: the Rust compiler
# (image tag and digest, and RUSTUP_TOOLCHAIN so `rust-toolchain.toml`'s
# `stable` channel cannot pull a newer one), the stellar CLI (version and
# tarball sha256), the platform, the source path and, through `--locked`, the
# dependency versions in Cargo.lock.
#
# Usage (see docs/building/build-and-test.md):
#   docker build --output type=local,dest=out .
#   cat out/wasm.sha256

FROM --platform=linux/amd64 rust:1.100.0-slim-bookworm@sha256:7c7d4eb4d05b7781187a327809466cfddf29753f6defdc3d19a0e291951b7a37 AS build

ARG STELLAR_CLI_VERSION=28.1.0
ARG STELLAR_CLI_SHA256=c1680deee94301d33ada7a17f98411e642a4248c727afbd2e43050d345746462

ENV RUSTUP_TOOLCHAIN=1.100.0

# curl fetches the CLI; the CLI binary links against libdbus and libudev.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl libdbus-1-3 libudev1 \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32v1-none

RUN curl -sSfL -o /tmp/stellar.tar.gz \
        "https://github.com/stellar/stellar-cli/releases/download/v${STELLAR_CLI_VERSION}/stellar-cli-${STELLAR_CLI_VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
    && echo "${STELLAR_CLI_SHA256}  /tmp/stellar.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/stellar.tar.gz -C /usr/local/bin stellar \
    && rm /tmp/stellar.tar.gz \
    && stellar --version \
    && rustc --version

WORKDIR /src
COPY . .

RUN stellar contract build --locked \
    && cd target/wasm32v1-none/release \
    && sha256sum circle.wasm reputation.wasm | tee /wasm.sha256

FROM scratch AS artifacts
COPY --from=build /src/target/wasm32v1-none/release/circle.wasm /
COPY --from=build /src/target/wasm32v1-none/release/reputation.wasm /
COPY --from=build /wasm.sha256 /
