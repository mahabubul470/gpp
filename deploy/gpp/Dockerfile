# gpp CLI image.
# Build:  docker build -f deploy/gpp/Dockerfile -t gpp .
# Use:    docker run --rm -v "$PWD:/work" -w /work gpp status
FROM rust:1-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake clang git && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release --bin gpp

FROM debian:stable-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 ca-certificates git && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/gpp /usr/local/bin/gpp
WORKDIR /work
ENTRYPOINT ["gpp"]
CMD ["--help"]
