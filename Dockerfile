FROM rust:1.93-slim-bookworm AS build
WORKDIR /mono
COPY --from=samp . ./rust
COPY . ./references/mirror-template
WORKDIR /mono/references/mirror-template
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=build /mono/references/mirror-template/target/release/samp-mirror /usr/local/bin/
ENTRYPOINT ["samp-mirror"]
