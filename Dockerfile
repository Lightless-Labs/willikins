# willikins-server, for Railway (task 12 of
# docs/plans/2026-09-12-milestone-2-providers-apply-mcp.md).
#
# Multi-stage: `cargo-chef` caches dependency compilation in its own
# layer (docs/research/2026-09-12-m2-dependencies.md, section 4,
# "Minimal multi-stage Dockerfile for a Rust binary"), then the real
# build, then a distroless runtime with only the one binary and the two
# shipped workflow documents.
#
# Base image: the `cargo-chef` maintainers publish a prebuilt image
# tagged `<chef version>-rust-<rust tag>`, built FROM the matching
# `rust:<tag>` image, so one `FROM` gets both the pinned Rust toolchain
# and `cargo-chef` with no extra install step. Verified against the
# registry before writing this (2026-09-15):
#   curl -s 'https://hub.docker.com/v2/repositories/lukemathwalker/cargo-chef/tags?page_size=100' \
#     | grep -o '"name":"[^"]*rust-1.97-slim-bookworm[^"]*"'
#   -> "0.1.78-rust-1.97-slim-bookworm" exists.
# `rust:1.97-slim-bookworm` itself was confirmed present the same way
# against `library/rust`'s tag list.
FROM lukemathwalker/cargo-chef:0.1.78-rust-1.97-slim-bookworm AS chef
WORKDIR /app

# ---------------------------------------------------------------------
# Planner: compute the dependency recipe from the workspace's
# Cargo.toml/Cargo.lock files only. `cargo chef prepare` reads every
# manifest to build the graph; it does not need the crates' actual
# source, but `COPY . .` is simplest and the layer is thrown away after
# `recipe.json` is produced.
# ---------------------------------------------------------------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---------------------------------------------------------------------
# Builder: cook the dependency layer from the recipe (cached across
# builds whenever only application code changes), then build the one
# binary this image ships.
#
# The workspace's only native dependency is `ring` (confirmed with
# `cargo tree -i ring`, which shows exactly one path down through
# `rustls` -> `ureq` -> `willikins-providers-http`; no `aws-lc-sys`,
# `openssl-sys`, or `cmake` anywhere in the tree), which needs a C
# compiler and libc headers to build its assembly/C sources. `gcc` and
# `libc6-dev` are exactly that and nothing more -- `--no-install-
# recommends` so apt does not also pull `make`, `g++`, and `dpkg-dev`
# the way `build-essential` would, and the apt lists are removed in the
# same layer so they do not persist in the image.
# ---------------------------------------------------------------------
FROM chef AS builder
RUN apt-get update \
    && apt-get install -y --no-install-recommends gcc libc6-dev \
    && rm -rf /var/lib/apt/lists/*
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked -p willikins-server --bin willikins-server

# ---------------------------------------------------------------------
# Runtime: distroless, same Debian generation (bookworm) as the
# builder so glibc matches between the two stages.
#
# `willikins-providers-http`'s `ureq` pulls in `webpki-roots`
# (confirmed in Cargo.lock: no `rustls-native-certs` or
# `rustls-platform-verifier` anywhere in the tree), which bakes
# Mozilla's root certificates into the binary at compile time. TLS
# verification therefore needs no `ca-certificates` package and no
# system trust store at runtime, which is what makes
# `gcr.io/distroless/cc-debian12` (glibc + libgcc, no package manager,
# no shell, no CA bundle) sufficient rather than the heavier
# `distroless/base`.
#
# User: root, not the `:nonroot` (uid 65532) variant of this image.
# Railway's own volume docs, fetched verbatim 2026-09-15:
#   https://docs.railway.com/guides/volumes ->
#     "Volumes are mounted as the `root` user."
#     "If you run an image that uses a non-root user, you should set
#      the following variable on your service:
#      `RAILWAY_RUN_UID=0`"
# This task's brief forbids setting any variable beyond the five this
# task's own plan names, so `RAILWAY_RUN_UID` is not on hand to set;
# running the image as its default root user, rather than opting into
# `:nonroot`, is what keeps `/data` (the mounted journal volume)
# writable without it. An unlockable or uncreatable journal is startup
# refusal five, and would brick a deployment nobody can shell into to
# fix -- root avoids that outcome entirely rather than trading it for a
# variable this task cannot set.
FROM gcr.io/distroless/cc-debian12
COPY --from=builder /app/target/release/willikins-server /usr/local/bin/willikins-server
# Only the two real workflow documents -- never `workflows/fixtures/`,
# which `Butler::start` would scan flat alongside them and refuse to
# start on the first negative fixture it hits.
COPY workflows/*.yaml /app/workflows/
ENV WILLIKINS_WORKFLOWS_DIR=/app/workflows
ENTRYPOINT ["/usr/local/bin/willikins-server"]
CMD ["serve", "--http"]
