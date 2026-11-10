# syntax=docker/dockerfile:1
#
# Two images from one build:
#
#   docker build -t trunks .                       # default target: `distroless`
#   docker build -t trunks:stdio --target stdio .
#
# - `distroless`: the binary and nothing else. For upstreams reached over HTTP.
# - `stdio`: adds Node.js (`npx`) and uv (`uvx`, with Python) so that stdio upstreams
#   distributed as npm or PyPI packages can be started inside the container.

# ── build ─────────────────────────────────────────────────────────────────────────────
FROM rust:1.97-bookworm AS build
WORKDIR /src
# cmake: for the C parts of the TLS library on targets without a pre-generated build.
RUN apt-get update \
 && apt-get install -y --no-install-recommends cmake \
 && rm -rf /var/lib/apt/lists/*
# The compiler is the one pinned in rust-toolchain.toml, not whatever the base image has.
COPY rust-toolchain.toml ./
RUN rustup toolchain install
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked --package trunks --bin trunks \
 && install -D target/release/trunks /out/trunks \
 && mkdir /out/data

# ── stdio: Node.js + uv for stdio upstreams ───────────────────────────────────────────
FROM ghcr.io/astral-sh/uv:python3.13-bookworm-slim AS stdio
COPY --from=node:24-bookworm-slim /usr/local/bin/node /usr/local/bin/node
COPY --from=node:24-bookworm-slim /usr/local/lib/node_modules /usr/local/lib/node_modules
RUN ln -s ../lib/node_modules/npm/bin/npm-cli.js /usr/local/bin/npm \
 && ln -s ../lib/node_modules/npm/bin/npx-cli.js /usr/local/bin/npx \
 && apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates git libstdc++6 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 10001 trunks \
 && mkdir /data \
 && chown trunks:trunks /data
COPY --from=build /out/trunks /usr/local/bin/trunks
ENV TRUNKS_DATABASE_URL=sqlite:///data/trunks.db
# Inside a container the port mapping decides who can reach the server, so listen on every
# interface (development mode otherwise listens on the container's own loopback only).
ENV TRUNKS_BIND_HOST=0.0.0.0
USER trunks
WORKDIR /data
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/trunks"]
CMD ["start"]

# ── distroless (default) ──────────────────────────────────────────────────────────────
FROM gcr.io/distroless/cc-debian12:nonroot AS distroless
COPY --from=build /out/trunks /usr/local/bin/trunks
COPY --from=build --chown=65532:65532 /out/data /data
ENV TRUNKS_DATABASE_URL=sqlite:///data/trunks.db
# Inside a container the port mapping decides who can reach the server, so listen on every
# interface (development mode otherwise listens on the container's own loopback only).
ENV TRUNKS_BIND_HOST=0.0.0.0
WORKDIR /data
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/trunks"]
CMD ["start"]
