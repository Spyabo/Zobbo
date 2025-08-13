# syntax=docker/dockerfile:1

# --- Builder stage ---
FROM rust:1.79 as builder
WORKDIR /app

# Cache dependencies
COPY backend/Cargo.toml backend/Cargo.lock /app/backend/
RUN mkdir -p /app/backend/src \
    && echo "fn main() {}" > /app/backend/src/main.rs \
    && cd /app/backend \
    && cargo build --release \
    && rm -f /app/backend/src/main.rs

# Build application
COPY . /app
RUN cd /app/backend && cargo build --release

# --- Runtime stage ---
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# App layout: keep working directory at backend so ../frontend resolves
WORKDIR /app/backend
COPY --from=builder /app/backend/target/release/zobbo-backend /usr/local/bin/zobbo-backend
COPY --from=builder /app/frontend /app/frontend

ENV RUST_LOG=info
EXPOSE 8080

CMD ["zobbo-backend"]
