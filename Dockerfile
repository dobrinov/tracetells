# The Trace server: one binary, run as an unprivileged user.
#   docker build -t trace .
#   docker run -e TRACE_DB=postgres://... -p 4318:4318 trace
# Production on a host with a shared reverse proxy and Postgres: see deploy/README.md.

FROM rust:1-slim-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# The MCP server serves the Claude Code plugin's commands as prompts.
COPY plugins plugins
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p trace-server \
 && cp target/release/trace /usr/local/bin/trace

FROM debian:trixie-slim
# curl only for the container healthcheck.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data --create-home trace
COPY --from=build /usr/local/bin/trace /usr/local/bin/trace
USER trace
WORKDIR /data
ENV TRACE_ADDR=0.0.0.0:4318
EXPOSE 4318
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS http://localhost:4318/api/auth/state > /dev/null || exit 1
ENTRYPOINT ["trace"]
CMD ["serve"]
