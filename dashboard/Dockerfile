FROM rust:1-bookworm AS build
WORKDIR /build
COPY Cargo.toml ./
COPY Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates wget \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 1000 app \
 && mkdir -p /data && chown app:app /data
WORKDIR /app
COPY --from=build /build/target/release/anyps5-live-dashboard /usr/local/bin/dashboard
COPY public ./public
ENV DATA_DIR=/data PORT=8080 PUBLIC_DIR=/app/public
USER app
VOLUME /data
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=90s --retries=3 CMD wget -qO- http://127.0.0.1:8080/healthz >/dev/null || exit 1
CMD ["dashboard"]
