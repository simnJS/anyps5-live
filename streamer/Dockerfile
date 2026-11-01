FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
      git pkg-config libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
 && rm -rf /var/lib/apt/lists/*
# The pinned GStreamer 1.22 plugin predates rustc's raw-pointer autoref lint.
RUN git clone --depth 1 --branch gstreamer-1.22.9 https://github.com/GStreamer/gst-plugins-rs.git /gst-plugins-rs \
 && RUSTFLAGS="-A dangerous_implicit_autorefs" cargo build --locked --release --manifest-path /gst-plugins-rs/audio/audiofx/Cargo.toml --lib \
 && strip --strip-unneeded /gst-plugins-rs/target/release/libgstrsaudiofx.so
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      chromium xvfb \
      fonts-inter fonts-noto-color-emoji fonts-dejavu-core \
      ca-certificates curl tini \
      libgstreamer1.0-0 libgstreamer-plugins-base1.0-0 \
      gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
      gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --create-home --uid 1000 streamer \
 && mkdir -p /music /recordings \
 && chown streamer:streamer /recordings
COPY --from=build /build/target/release/anyps5-live-streamer /usr/local/bin/anyps5-live-streamer
COPY --from=build /gst-plugins-rs/target/release/libgstrsaudiofx.so /usr/local/lib/gstreamer-1.0/libgstrsaudiofx.so
COPY --from=build /gst-plugins-rs/LICENSE-MPL-2.0 /usr/share/licenses/gst-plugin-audiofx/LICENSE-MPL-2.0
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
COPY shorts.sh /usr/local/bin/shorts.sh
RUN chmod 0755 /usr/local/bin/entrypoint.sh /usr/local/bin/shorts.sh
USER streamer
ENV DASHBOARD_URL=http://dashboard:8080/ WIDTH=2560 HEIGHT=1440 \
    FPS=30 CAPTURE_FPS=30 VIDEO_BITRATE=12000k AUDIO_BITRATE=128k \
    PRESET=superfast WARMUP_SECONDS=15 BROWSER_RESTART_HOURS=24 \
    GST_PLUGIN_PATH_1_0=/usr/local/lib/gstreamer-1.0
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/entrypoint.sh"]
