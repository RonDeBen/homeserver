# syntax=docker/dockerfile:1
#
# One image, all binaries. Each compose service runs a different one via its
# `command` (orchestrator, calendar serve, ...). Multi-stage: a full Rust
# toolchain builds; a slim Debian runs. ring/rustls link statically, so the
# runtime needs no OpenSSL or extra libs.

FROM rust:1-slim-bookworm AS builder
WORKDIR /app
COPY . .
# Cache the registry and target dir across builds. Because a cache mount isn't
# part of the image layer, copy the finished binaries out to /out so the
# runtime stage can COPY them.
RUN --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/usr/local/cargo/registry \
    mkdir -p /out && \
    cargo build --release --bin orchestrator --bin calendar --bin hsctl --bin gateway && \
    cp target/release/orchestrator target/release/calendar target/release/hsctl target/release/gateway /out/

FROM debian:bookworm-slim AS runtime
WORKDIR /app
COPY --from=builder /out/orchestrator /out/calendar /out/hsctl /out/gateway /usr/local/bin/
# No ENTRYPOINT: compose sets `command` per service. WORKDIR /app so the
# orchestrator's default schedules path (deploy/schedules.toml) resolves against
# the file compose mounts there.
