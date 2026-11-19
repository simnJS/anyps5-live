# Single-container image: the dashboard and the streamer in one container, for platforms that
# build one Dockerfile and run one service. docker-compose.yml runs them as two containers instead.
FROM rust:1-bookworm AS build
WORKDIR /build
COPY dashboard/Cargo.toml dashboard/Cargo.lock ./dashboard/
# The dependencies get their own layer: changing the sources does not recompile them.
RUN mkdir dashboard/src && echo 'fn main() {}' > dashboard/src/main.rs \
 && cargo build --locked --release --manifest-path dashboard/Cargo.toml \
 && rm -r dashboard/src
COPY dashboard/src ./dashboard/src
RUN touch dashboard/src/main.rs && cargo build --locked --release --manifest-path dashboard/Cargo.toml

FROM node:22-bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      chromium xvfb x11-utils ffmpeg \
      fonts-inter fonts-noto-color-emoji fonts-dejavu-core \
      ca-certificates curl tini \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /build/dashboard/target/release/anyps5-live-dashboard /usr/local/bin/dashboard
COPY dashboard/public ./public
COPY streamer/entrypoint.sh /usr/local/bin/streamer.sh
COPY streamer/shorts.sh /usr/local/bin/shorts.sh
COPY start.sh /usr/local/bin/start.sh
RUN chmod 0755 /usr/local/bin/streamer.sh /usr/local/bin/shorts.sh /usr/local/bin/start.sh \
 && mkdir -p /data /music /recordings /gameplay \
 && chown node:node /data /recordings

ENV DATA_DIR=/data \
    PUBLIC_DIR=/app/public \
    GAMEPLAY_DIR=/gameplay \
    PORT=3000 \
    WIDTH=2560 \
    HEIGHT=1440 \
    FPS=30 \
    CAPTURE_FPS=30 \
    VIDEO_BITRATE=12000k \
    BUFSIZE=24000k \
    AUDIO_BITRATE=128k \
    PRESET=superfast \
    BROWSER_RESTART_HOURS=24

VOLUME /data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=90s --retries=3 \
  CMD curl -fsS "http://127.0.0.1:${PORT}/healthz" >/dev/null || exit 1
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/start.sh"]
