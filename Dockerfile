# gpp MCP server image (repo-root Dockerfile = what MCP directories such as
# Glama build and probe).
# Builds the CLI from this checkout and pre-initializes a Graphex-enabled
# repo at /work so `gpp mcp-server --stdio` starts and answers
# initialize / tools/list with no volume mounted. For real use, mount your
# project at /work instead (see deploy/gpp/Dockerfile).
#
# Build:  docker build -t gpp-mcp .
# Probe:  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
#           '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | docker run -i --rm gpp-mcp
FROM rust:1-slim AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config git && rm -rf /var/lib/apt/lists/*
COPY . .
RUN cargo build --release --bin gpp --locked

FROM debian:stable-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates git && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/gpp /usr/local/bin/gpp
ENV HOME=/root
WORKDIR /work
# Seed a repo so the server has something to serve out of the box.
RUN gpp init --graphex \
 && printf '# demo\n' > README.md
ENTRYPOINT ["gpp", "mcp-server", "--stdio"]
