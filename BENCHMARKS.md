# Rust rebuild benchmark

Measured against upstream `main` at `17da608`, the base commit of this PR, on the same Docker Desktop 29.8.2 Linux x86_64 host. The host reports 20 vCPUs and 15.6 GiB to Docker. Both images ran without GitHub or YouTube credentials or a live stream key. The old benchmark figures in earlier revisions used sparse CPU sampling and inconsistent encode settings; they have been discarded.

## 1440p encode

The offline encode benchmark used the production video profile (2560×1440, 30 fps, 12,000 kbit/s target, 24,000 kbit VBV, `superfast`) under a four-CPU container limit. Each implementation ran three 15-second finite MP4 jobs. Results below are medians unless noted. The script reads `cpu.stat` and `memory.peak` from cgroup v2 after the job and counts output frames from each MP4.

Encode runs use the same generated 440 Hz, 44.1 kHz stereo WAV track for both images.

| Measure | Upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Container image | 1,596.3 MiB | 1,400.6 MiB | 12.3% smaller |
| Encode job wall time | 34.10 s | 31.58 s | 7.4% faster |
| Cgroup CPU time | 14.01 s | 9.58 s | 31.6% lower |
| Peak cgroup memory | 1,678.2 MiB | 800.0 MiB | 52.3% lower |
| Encoded MP4 size | 21.42 MiB | 21.41 MiB | 0.05% smaller |
| Output frames | 450 / 30 fps | 450 / 30 fps | Same |

Individual wall times were 33.99, 35.02, and 34.10 seconds for upstream and 31.58, 31.64, and 31.58 seconds for Rust. CPU time and peak memory are cgroup counters, not sampled `docker stats` estimates. The Rust encoder now sizes its x264 worker pool from the container's available parallelism.

## Dashboard and idle footprint

The dashboard pass used the same images on the same host. Figures below are medians across four paired passes (the dashboard pass from the full benchmark plus three additional passes); each pass measured readiness once per image and API latency over 40 timed `/api/state` requests after 5 warm-ups.

| Measure | Upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Health-ready time | 0.852 s | 0.449 s | 47.3% faster |
| Idle memory | 65.39 MiB | 61.59 MiB | 5.8% lower |
| `/api/state` median | 1.60 ms | 0.77 ms | 51.9% lower |
| `/api/state` p95 | 2.13 ms | 1.02 ms | 52.2% lower |
| `/api/state` response | 41,410 bytes | 42,012 bytes | 1.5% larger |

The measured API response was 1.5% larger; serialized content includes dynamic source metadata and timestamps. Idle-memory samples varied substantially between repeat runs, so the median is shown and should not be read as a tight bound. API timings are short local requests and do not predict remote network latency.

The final image also removes Chromium's unused Vulkan validation layer and mock ICD binaries while retaining the SwiftShader runtime. This reduced the image by 28.8 MiB from the previous Rust image. A finite 720p GStreamer capture still completed with 90 frames, and the dashboard health and static routes returned HTTP 200.

## GStreamer video path

The live capture already matches the configured Xvfb output dimensions. The pipeline now skips the redundant `videoscale` stage, parallelizes `videoconvert` using the container-aware worker count, and inserts `videorate` only when `CAPTURE_FPS` differs from `FPS`. A finite 60-to-30 fps smoke capture produced 121 frames at 30 fps over the four-second recording window.

Compared with the previous Rust image (`04217e4`), three paired finite encodes kept the same 450 frames and 21.41 MiB output. Median wall time moved from 31.90 to 31.58 seconds (1.0% faster), cgroup CPU from 11.89 to 10.99 seconds (7.6% lower), and peak memory from 839.8 to 800.0 MiB (4.7% lower). These are modest differences and should be treated as directional; the primary gain remains the full comparison against FFmpeg above.

## Dashboard hot-path refactor

Compared with the previous Rust revision (`c70fa35`), the new collector and response hot paths were measured in three paired 40-request dashboard passes and five paired encode runs. Medians of the three dashboard passes show `/api/state` median latency improving from 1.29 ms to 1.02 ms and p95 from 1.61 ms to 1.29 ms. Response size matched in every paired pass (42,270 or 42,273 bytes). Median health readiness was 1.481 vs 1.489 seconds. The five encode runs had medians of 34.13 vs 34.16 seconds, 8.27 vs 8.34 cgroup CPU seconds, and 728.5 vs 737.3 MiB peak cgroup memory; every output remained 21.41 MiB with all 450 frames. Those encode differences are below 1.3% and the video pipeline itself is unchanged.

The dashboard changes cache game/report matching expressions instead of compiling them for each discussion or chat command, replace repeated Roman numeral regex passes with equivalent word-token lookup, and serialize the borrowed state snapshot without deep-cloning its 44 KB JSON tree for every poll. Unit tests check the game-stage/report behavior and response JSON parity.

## Audio smoke check

A separate finite MP4 smoke check with a synthetic 44.1 kHz stereo WAV soundtrack completed successfully. `ffprobe` identified H.264 video and AAC audio at 44.1 kHz stereo, and the output was non-empty with the requested finite duration. This checks the local music path and muxing; it does not validate an external RTMP endpoint or a 24-hour live run.

## Reproduction

From the repository root:

```powershell
git worktree add .bench-upstream 17da608
docker build -t anyps5-live-upstream-baseline .bench-upstream
git worktree remove .bench-upstream
docker build -t anyps5-live-rust .
pwsh -NoProfile -File benchmarks/New-BenchmarkTone.ps1
pwsh -NoProfile -File benchmarks/bench-containers.ps1 -Samples 40 -EncodeSeconds 15 -EncodeRuns 3 -AudioTrack benchmark-tone.wav
```

The tone generator creates a deterministic 45-second 440 Hz, 44.1 kHz stereo WAV. The WAV is a local benchmark artifact and is not committed.

The benchmark script records cgroup CPU and peak-memory counters, output frame count, dashboard readiness and API latency. A host `ffprobe` executable is used to inspect MP4s; the application runtime does not depend on FFmpeg. Live YouTube chat, reminders, Short upload, external RTMP delivery, and 24-hour reconnect behavior require credentials or an external service and remain unverified here.
