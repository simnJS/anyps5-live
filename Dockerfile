# Single-container image: the dashboard and the streamer in one container, for platforms that
# build one Dockerfile and run one service. docker-compose.yml runs them as two containers instead.
FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
      git pkg-config libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
 && rm -rf /var/lib/apt/lists/*
# The pinned GStreamer 1.22 plugin predates rustc's raw-pointer autoref lint.
RUN git clone --depth 1 --branch gstreamer-1.22.9 https://github.com/GStreamer/gst-plugins-rs.git /gst-plugins-rs \
 && RUSTFLAGS="-A dangerous_implicit_autorefs" cargo build --release --manifest-path /gst-plugins-rs/audio/audiofx/Cargo.toml --lib \
 && strip --strip-unneeded /gst-plugins-rs/target/release/libgstrsaudiofx.so
WORKDIR /build
COPY dashboard/Cargo.toml ./dashboard/Cargo.toml
COPY dashboard/Cargo.lock ./dashboard/Cargo.lock
COPY dashboard/src ./dashboard/src
RUN cargo build --release --manifest-path dashboard/Cargo.toml
COPY streamer/Cargo.toml ./streamer/Cargo.toml
COPY streamer/Cargo.lock ./streamer/Cargo.lock
COPY streamer/src ./streamer/src
RUN cargo build --release --manifest-path streamer/Cargo.toml

FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      chromium xvfb x11-utils \
      fonts-inter fonts-noto-color-emoji fonts-dejavu-core \
      ca-certificates curl tini util-linux \
      libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 \
      gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
      gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=build /build/dashboard/target/release/anyps5-live-dashboard /usr/local/bin/dashboard
COPY --from=build /build/streamer/target/release/anyps5-live-streamer /usr/local/bin/anyps5-live-streamer
COPY --from=build /gst-plugins-rs/target/release/libgstrsaudiofx.so /usr/local/lib/gstreamer-1.0/libgstrsaudiofx.so
COPY --from=build /gst-plugins-rs/LICENSE-MPL-2.0 /usr/share/licenses/gst-plugin-audiofx/LICENSE-MPL-2.0
COPY dashboard/public ./public
COPY streamer/entrypoint.sh /usr/local/bin/streamer.sh
COPY streamer/shorts.sh /usr/local/bin/shorts.sh
COPY start.sh /usr/local/bin/start.sh
RUN chmod 0755 /usr/local/bin/streamer.sh /usr/local/bin/shorts.sh /usr/local/bin/start.sh \
 && mkdir -p /data /music /recordings \
 && useradd --create-home --uid 1000 app \
 && chown app:app /data /recordings

ENV DATA_DIR=/data \
    PORT=3000 \
    PUBLIC_DIR=/app/public \
    WIDTH=2560 \
    HEIGHT=1440 \
    FPS=30 \
    CAPTURE_FPS=30 \
    VIDEO_BITRATE=12000k \
    BUFSIZE=24000k \
    AUDIO_BITRATE=128k \
    PRESET=superfast \
    WARMUP_SECONDS=15 \
    BROWSER_RESTART_HOURS=24 \
    GST_PLUGIN_PATH_1_0=/usr/local/lib/gstreamer-1.0

VOLUME /data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=90s --retries=3 \
  CMD curl -fsS "http://127.0.0.1:${PORT}/healthz" >/dev/null || exit 1
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/start.sh"]
