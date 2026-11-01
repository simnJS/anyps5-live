# Rust rebuild benchmark

Measured against upstream `main` at `17da608`, the base commit of this PR, on the same Docker Desktop 29.8.2 Linux x86_64 host. The host reports 20 vCPUs and 15.6 GiB to Docker. Both images ran without GitHub or YouTube credentials or a live stream key. The old benchmark figures in earlier revisions used sparse CPU sampling and inconsistent encode settings; they have been discarded.

## 1440p encode

The offline encode benchmark used the production video profile (2560×1440, 30 fps, 12,000 kbit/s target, 24,000 kbit VBV, `superfast`) under a four-CPU container limit. Each implementation ran three 15-second finite MP4 jobs. Results below are medians unless noted. The script reads `cpu.stat` and `memory.peak` from cgroup v2 after the job and counts output frames from each MP4.

Encode runs use the same generated 440 Hz, 44.1 kHz stereo WAV track for both images.

| Measure | Upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Container image | 1,596.3 MiB | 1,429.3 MiB | 10.5% smaller |
| Encode job wall time | 34.72 s | 31.52 s | 9.2% faster |
| Cgroup CPU time | 19.81 s | 10.00 s | 49.5% lower |
| Peak cgroup memory | 1,676.4 MiB | 811.9 MiB | 51.6% lower |
| Encoded MP4 size | 21.42 MiB | 21.41 MiB | 0.05% smaller |
| Output frames | 450 / 30 fps | 450 / 30 fps | Same |

Individual wall times were 34.88, 34.72, and 33.95 seconds for upstream and 31.32, 31.88, and 31.52 seconds for Rust. CPU time and peak memory are cgroup counters, not sampled `docker stats` estimates. The Rust encoder now sizes its x264 worker pool from the container's available parallelism.

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
