FROM rust:1.86-slim AS build
WORKDIR /build
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=build /build/target/release/samp-mirror /usr/local/bin/
ENTRYPOINT ["samp-mirror"]
