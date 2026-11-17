# syntax=docker/dockerfile:1

# =============================================================================
# Stage 1: Builder
# =============================================================================
FROM rust:1-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libxkbcommon-dev \
    libxkbcommon-x11-dev \
    libgtk-3-dev \
    libgl1-mesa-dev \
    libegl1-mesa-dev \
    libfontconfig1-dev \
    libfreetype-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/rust-sql
COPY . .

RUN cargo build --release --locked

# =============================================================================
# Stage 2: Runtime (Web UI / TUI / Headless CLI)
# =============================================================================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    libsqlite3-0 \
    libxcb1 \
    libxcb-render0 \
    libxcb-shape0 \
    libxcb-xfixes0 \
    libxkbcommon0 \
    libxkbcommon-x11-0 \
    libgtk-3-0 \
    libgl1 \
    libegl1 \
    libfontconfig1 \
    libfreetype6 \
    openssh-client \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/rust-sql/target/release/Rust-SQL /usr/local/bin/Rust-SQL

EXPOSE 8080
ENTRYPOINT ["Rust-SQL"]
# Needs TLS (web.tlsCertPath / web.tlsKeyPath in /data/rust-sql.json) and an admin account;
# see docs/app/web-server.md.
CMD ["--web", "--bind", "0.0.0.0", "--port", "8080"]
