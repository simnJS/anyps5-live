# Rust rebuild benchmark

Measured against upstream `main` at `17da608`, the base commit of this PR, on the same Docker Desktop 29.8.2 Linux x86_64 host. The host reports 20 vCPUs and 15.6 GiB to Docker. Both images ran without GitHub or YouTube credentials or a live stream key. The old benchmark figures in earlier revisions used sparse CPU sampling and inconsistent encode settings; they have been discarded.

## 1440p encode

The offline encode benchmark used the production video profile (2560×1440, 30 fps, 12,000 kbit/s target, 24,000 kbit VBV, `superfast`) under a four-CPU container limit. Each implementation ran three 15-second finite MP4 jobs. Results below are medians unless noted. The script reads `cpu.stat` and `memory.peak` from cgroup v2 after the job and counts output frames from each MP4.

Encode runs use the same generated 440 Hz, 44.1 kHz stereo WAV track for both images.

| Measure | Upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Container image | 1,596.3 MiB | 1,429.1 MiB | 10.5% smaller |
| Encode job wall time | 34.22 s | 34.01 s | 0.61% faster |
| Cgroup CPU time | 13.36 s | 8.11 s | 39.3% lower |
| Peak cgroup memory | 1,689.8 MiB | 741.1 MiB | 56.1% lower |
| Encoded MP4 size | 21.42 MiB | 21.41 MiB | 0.05% smaller |
| Output frames | 450 / 30 fps | 450 / 30 fps | Same |

Individual wall times were 34.07, 33.92, and 34.26 seconds for upstream and 33.92, 33.95, and 34.10 seconds for Rust. CPU time and peak memory are cgroup counters, not sampled `docker stats` estimates. The Rust encoder now sizes its x264 worker pool from the container's available parallelism.

## Dashboard and idle footprint

The dashboard pass used the same images on the same host. Readiness is measured once per implementation; API latency uses 5 warm-ups and 40 timed `/api/state` requests.

| Measure | Upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Health-ready time | 0.902 s | 1.426 s | 0.524 s slower |
| Idle memory | 102.8 MiB | 17.66 MiB | 82.8% lower |
| `/api/state` median | 1.16 ms | 0.98 ms | 15.5% lower |
| `/api/state` p95 | 1.52 ms | 1.17 ms | 23.0% lower |
| `/api/state` response | 41,410 bytes | 42,273 bytes | 2.1% larger |

The readiness measurement is a remaining regression and is included explicitly. API timings are short local requests and do not predict remote network latency.

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
