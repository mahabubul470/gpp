# gpp-relay — always-on sync hub.
# Build:  docker build -f deploy/relay/Dockerfile -t gpp-relay .
# Run:    docker run -p 9473:9473 -p 9474:9474 -v gpp-data:/data gpp-relay
FROM rust:1-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev cmake clang git && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release --bin gpp-relay

FROM debian:stable-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    libssl3 ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/gpp-relay /usr/local/bin/gpp-relay
VOLUME /data
EXPOSE 9473 9474
ENTRYPOINT ["gpp-relay", "--port", "9473", "--storage", "/data"]
