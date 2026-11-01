# Rust rebuild benchmark

Measured against upstream `main` at `17da608`, the exact base commit of this PR. Results are medians from two paired runs on the same Docker Desktop 29.8.2 Linux x86_64 host (20 vCPU, 15.6 GiB reported to Docker). Both images ran without GitHub or YouTube credentials or a live stream key.

| Measure | Current upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Container image | 1,596.3 MiB | 1,429.0 MiB | 10.5% smaller |
| Health-ready after `docker run` | 0.77 s | 1.43 s | 0.66 s slower |
| Idle container memory | 64.68 MiB | 17.62 MiB | 72.8% lower |
| `/api/state` median (40 warm requests) | 1.21 ms | 0.86 ms | 29.0% lower |
| `/api/state` p95 | 1.67 ms | 1.19 ms | 29.0% lower |
| `/api/state` payload | 41,410 bytes | 42,273 bytes | 2.1% larger |
| 15-second 1280×720, 30 fps MP4 job, end-to-end wall time | 34.55 s | 34.03 s | 1.5% faster |
| Encoded MP4 size | 5.32 MiB | 5.55 MiB | 4.3% larger |
| Mean container CPU during MP4 job | 12.05% | 10.40% | 1.65 percentage points lower |
| Peak container memory during MP4 job | 624.3 MiB | 244.4 MiB | 60.9% lower |

The encode job used the same `superfast` x264 preset, 3000 kbit/s target, 6000 kbit VBV, 1280×720 output, 30 fps, and 15-second recording duration. Wall time includes container startup, browser warmup, recording, and mux finalization. The Rust job was 0.52 seconds faster at the median. Its output was 0.23 MiB larger, and its health-ready time was 0.66 seconds slower. The two runs used the same state payload size for each implementation; remote upstream content can still affect future runs.

## Production resolution comparison

The production profile uses the repository defaults: 2560×1440, 30 fps, 12,000 kbit/s target, 24,000 kbit VBV, `superfast`, a four-CPU container limit, and a 15-second finite MP4 recording. The table reports three paired encode runs on the same images. Dashboard readiness, idle memory, and API values come from one additional same-host pass:

| Measure | Current upstream Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Image | 1,596.3 MiB | 1,429.0 MiB | 10.5% smaller |
| Health-ready | 0.86 s | 1.79 s | 0.93 s slower |
| Idle memory | 80.91 MiB | 17.29 MiB | 78.6% lower |
| `/api/state` median / p95 (10 requests) | 1.41 / 2.14 ms | 0.81 / 0.96 ms | 42.6% / 55.1% lower |
| Encoded recording size (median of 3) | 21.19 MiB | 21.51 MiB | 1.5% larger |
| Encode job wall time (median of 3) | 34.06 s | 34.83 s | 2.3% slower |
| Mean container CPU during encode (median of 3) | 35.7% | 19.0% | 46.8% lower |
| Peak memory during encode (median of 3) | 1,581.1 MiB | 1,675.3 MiB | 5.9% higher |

Wall time includes container start, Chromium warmup, recording, and mux finalization. The Rust MP4 was inspected with `ffprobe`: H.264, 2560×1440, TV range, BT.709 primaries/transfer/matrix, and keyframes every two seconds. Its average frame rate was close to 30 fps. The live YouTube/RTMP path still needs credentials for an end-to-end check. Image size, idle memory, API latency, and encode CPU improve in this profile; health-ready time, encode wall time, file size, and peak encode memory remain modest regressions and are reported as measured.

The benchmark script waits for both progress and history collection before measuring `/api/state`, then takes 5 warm-up requests and 40 timed requests. It reports the response size so changes in collected data remain visible. Health readiness remains governed by the dashboard's collected-data contract.

To reproduce from the repository root:

```powershell
git worktree add .bench-upstream 17da608
docker build -t anyps5-live-upstream-baseline .bench-upstream
git worktree remove .bench-upstream
docker build -t anyps5-live-rust .
pwsh -NoProfile -File benchmarks/bench-containers.ps1 -Samples 40 -EncodeSeconds 15 -EncodeRuns 3
```

The offline finite-recording benchmark uses silence. A separate smoke recording with a WAV soundtrack confirmed that the bundled Rust `audioloudnorm` plugin loads and produces a non-empty MP4. The live YouTube chat, reminders, Short upload, and RTMP delivery require credentials or an external service and were not exercised in this benchmark.
