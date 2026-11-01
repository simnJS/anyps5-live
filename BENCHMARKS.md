# Rust rebuild benchmark

Measured 1 November 2026 against the original fork at `7bfdacc8a1bdbab235624623241b85b981803992`. Both images were built and run on the same Docker Desktop 29.8.2 Linux x86_64 engine (20 vCPU, 15.6 GiB reported to Docker), with no GitHub or YouTube credentials and no live stream key.

| Measure | Original Node + FFmpeg | Rust + GStreamer | Change |
|---|---:|---:|---:|
| Container image | 1,574.1 MiB | 1,432.6 MiB | 9.0% smaller |
| Health-ready after `docker run` | 918 ms | 430 ms | 53.2% faster |
| Idle container memory | 87.51 MiB | 25.71 MiB | 70.6% lower |
| `/api/state` latency, median (40 warm requests) | 1.22 ms | 0.90 ms | 26.2% lower |
| `/api/state` latency, p95 | 2.61 ms | 1.19 ms | 54.4% lower |
| 15-second 1280×720, 30 fps MP4 job, end-to-end wall time | 34.17 s | 34.47 s | 0.9% slower |
| Encoded MP4 size | 5.32 MiB | 5.55 MiB | 4.3% larger |
| Mean container CPU during MP4 job | 16.0% | 14.4% | −1.6 percentage points |
| Peak container memory during MP4 job | 591.9 MiB | 358.6 MiB | 39.4% lower |

The MP4 job used the same `superfast` x264 preset, 3000 kbit/s target, 6000 kbit VBV, 1280×720 output, 30 fps, and 15-second recording duration. The measured wall time includes container startup and the matching 15-second browser warmup. Output size is close between encoders; this run was 0.3 seconds slower on GStreamer, used 1.6 percentage points less average CPU, and cut peak memory by 39.4%.

Dashboard readiness and idle-memory results are single-run measurements. HTTP latency excludes the first five warm-up requests. The benchmark script runs the same requests and encoding jobs against both image tags and reports raw measurements:

```powershell
pwsh -NoProfile -File benchmarks/bench-containers.ps1 -Samples 40 -EncodeSeconds 15
```
