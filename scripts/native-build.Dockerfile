# SPDX-License-Identifier: MIT
# Ubuntu 24.04 keeps the distribution ABI at glibc 2.39. The Rust toolchain
# and locked registry sources are supplied read-only when running this image.
FROM ubuntu:24.04@sha256:534baea6a22c03a63003dbc8dbe78fe34bc0d7e595d9a9dc9834884ff530eb55
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config python3 ca-certificates woff2 liblzma-dev \
    libxkbcommon-dev libwayland-dev libx11-dev libxrandr-dev libxi-dev \
    && rm -rf /var/lib/apt/lists/*
ENV PATH=/opt/rust/bin:/usr/local/bin:/usr/bin:/bin CARGO_HOME=/cargo CARGO_TARGET_DIR=/build/target
WORKDIR /src
