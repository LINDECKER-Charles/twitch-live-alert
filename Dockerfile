FROM rust:1.99.1-bookworm AS build
WORKDIR /build

COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && install -d -o 10001 -g 10001 /app/data

WORKDIR /app
COPY --from=build /build/target/release/alert-bloodborne-fr-live /usr/local/bin/alert-bloodborne-fr-live
USER 10001:10001
ENTRYPOINT ["/usr/local/bin/alert-bloodborne-fr-live"]
