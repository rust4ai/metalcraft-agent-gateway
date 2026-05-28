FROM rust:1.91-slim AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Cache dependency build
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

COPY src ./src
RUN touch src/main.rs && cargo build --release

# ── Runtime ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/metalcraft-agent-gateway /usr/local/bin/metalcraft-agent-gateway

ENV PORT=3000
EXPOSE 3000

CMD ["metalcraft-agent-gateway"]
