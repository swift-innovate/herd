FROM rust:1.75 as builder

WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/herd /usr/local/bin/herd

EXPOSE 40114

ENTRYPOINT ["herd"]
CMD ["--config", "/etc/herd/herd.yaml"]