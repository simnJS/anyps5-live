# Rust rebuild benchmark

Measured against upstream `main` at `17da608`, the base commit of this PR, on the same Docker Desktop 29.8.2 Linux x86_64 host. The host reports 20 vCPUs and 15.6 GiB to Docker. Both images ran without GitHub or YouTube credentials or a live stream key. The old benchmark figures in earlier revisions used sparse CPU sampling and inconsistent encode settings; they have been discarded.

## 1440p encode

The offline encode benchmark used the production video profile (2560×1440, 30 fps, 12,000 kbit/s target, 24,000 kbit VBV, `superfast`) under a four-CPU container limit. Each implementation ran three 15-second finite MP4 jobs. Results below are medians unless noted. The script reads `cpu.stat` and `memory.peak` from cgroup v2 after the job and counts output frames from each MP4.

| Measure | Upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Container image | 1,596.3 MiB | 1,429.1 MiB | 10.5% smaller |
| Encode job wall time | 34.07 s | 33.95 s | 0.35% faster |
| Cgroup CPU time | 15.04 s | 8.93 s | 40.6% lower |
| Peak cgroup memory | 1,664.5 MiB | 733.0 MiB | 56.0% lower |
| Encoded MP4 size | 21.42 MiB | 21.41 MiB | 0.05% smaller |
| Output frames | 450 / 30 fps | 450 / 30 fps | Same |

Individual wall times were 34.07, 33.92, and 34.26 seconds for upstream and 33.92, 33.95, and 34.10 seconds for Rust. CPU time and peak memory are cgroup counters, not sampled `docker stats` estimates. The Rust encoder now sizes its x264 worker pool from the container's available parallelism.

## Dashboard and idle footprint

The dashboard pass used the same images on the same host. Readiness is measured once per implementation; API latency uses 5 warm-ups and 40 timed `/api/state` requests.

| Measure | Upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Health-ready time | 0.874 s | 1.414 s | 0.540 s slower |
| Idle memory | 77.81 MiB | 18.79 MiB | 75.8% lower |
| `/api/state` median | 0.95 ms | 0.82 ms | 13.7% lower |
| `/api/state` p95 | 1.49 ms | 0.99 ms | 33.6% lower |
| `/api/state` response | 41,410 bytes | 42,273 bytes | 2.1% larger |

The readiness measurement is a remaining regression and is included explicitly. API timings are short local requests and do not predict remote network latency.

## Audio smoke check

A separate finite MP4 smoke check with a synthetic 44.1 kHz stereo WAV soundtrack completed successfully. `ffprobe` identified H.264 video and AAC audio at 44.1 kHz stereo, and the output was non-empty with the requested finite duration. This checks the local music path and muxing; it does not validate an external RTMP endpoint or a 24-hour live run.

## Reproduction

From the repository root:

```powershell
git worktree add .bench-upstream 17da608
docker build -t anyps5-live-upstream-baseline .bench-upstream
git worktree remove .bench-upstream
docker build -t anyps5-live-rust .
pwsh -NoProfile -File benchmarks/bench-containers.ps1 -Samples 40 -EncodeSeconds 15 -EncodeRuns 3 -AudioTrack benchmark-tone.wav
```

The benchmark script records cgroup CPU and peak-memory counters, output frame count, dashboard readiness and API latency. A host `ffprobe` executable is used to inspect MP4s; the application runtime does not depend on FFmpeg. Live YouTube chat, reminders, Short upload, external RTMP delivery, and 24-hour reconnect behavior require credentials or an external service and remain unverified here.
