FROM rust:1.91-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY facilitator/ facilitator/
COPY examples/ examples/
COPY patches/ patches/

# Build only the emei-server binary in release mode
RUN cargo build --release --bin emei-server -p emei-facilitator

FROM debian:bookworm-slim AS runner

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/emei-server /usr/local/bin/emei-server

ENV RUST_LOG=emei_facilitator=info

EXPOSE 8080

CMD ["emei-server"]
