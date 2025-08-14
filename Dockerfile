# Multi-stage Dockerfile for Zobbo (Rust + Axum)

# --- Builder ---
FROM rust:1-bookworm AS builder
WORKDIR /app

# Copy the whole game directory so Askama can resolve templates at compile time
COPY game ./game

# Build deps for any crates that might need system libs (keep minimal)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app/game/backend
RUN cargo build --release

# --- Runtime ---
FROM debian:bookworm-slim AS runtime
RUN useradd -m -u 10001 appuser
WORKDIR /app

# Copy the built binary
COPY --from=builder /app/game/backend/target/release/zobbo /app/zobbo

# Copy static assets used by the server at runtime
COPY --from=builder /app/game/frontend/static /app/game/frontend/static

ENV PORT=8080 \
    RUST_LOG=info
EXPOSE 8080
USER appuser

CMD ["/app/zobbo"]
