FROM rust:latest as builder

RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    zlib1g-dev \
    cmake \
    clang \
    libclang-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY . .

RUN cargo fetch

RUN cargo build --release --bin marketplace-api

FROM rust:1.77-slim-bookworm

RUN apt-get update && apt-get install -y libssl3

COPY --from=builder /app/target/release/marketplace-api /usr/local/bin/marketplace-api

EXPOSE 4000

CMD ["marketplace-api"]
