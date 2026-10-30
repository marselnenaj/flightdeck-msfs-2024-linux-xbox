# SPDX-License-Identifier: MIT
# Rebuild the Linux Xodus CLI/service at the supported glibc 2.39 baseline.
# Mount the official Rust 1.98 toolchain read-only at /opt/rust and use the
# locked, vendored source graph. Wine/Windows ABI components are separate.
FROM ubuntu:24.04@sha256:534baea6a22c03a63003dbc8dbe78fe34bc0d7e595d9a9dc9834884ff530eb55
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config cmake protobuf-compiler ca-certificates \
    libssl-dev libdbus-1-dev libgtk-3-dev libwebkit2gtk-4.1-dev \
    && rm -rf /var/lib/apt/lists/*
ENV PATH=/opt/rust/bin:/usr/local/bin:/usr/bin:/bin
WORKDIR /src
