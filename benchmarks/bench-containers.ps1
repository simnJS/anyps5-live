param(
  [string]$BaselineImage = "anyps5-live-upstream-baseline",
  [string]$CandidateImage = "anyps5-live-rust",
  [int]$Samples = 40,
  [int]$HostPort = 18081,
  [int]$EncodeSeconds = 15,
  [int]$Width = 2560,
  [int]$Height = 1440,
  [string]$VideoBitrate = "12000k",
  [string]$BufferSize = "24000k",
  [double]$CpuLimit = 4,
  [int]$EncodeRuns = 3,
  [string]$AudioTrack = ""
)

$ErrorActionPreference = "Stop"

function Get-Percentile([double[]]$Values, [double]$Percentile) {
  $sorted = @($Values | Sort-Object)
  $index = [Math]::Min($sorted.Count - 1, [Math]::Max(0, [Math]::Ceiling($Percentile * $sorted.Count) - 1))
  return [Math]::Round($sorted[$index], 2)
}

function Measure-Encode([string]$Image, [string]$Name) {
  $container = "anyps5-encode-$Name"
  try {
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $supervisor = 'set +e; /usr/bin/tini -s -- /usr/local/bin/start.sh; status=$?; awk ''/^usage_usec / {print $2}'' /sys/fs/cgroup/cpu.stat > /recordings/cpu.stat; cat /sys/fs/cgroup/memory.peak > /recordings/memory.peak; exit "$status"'
    $mountArgs = @()
    if ($AudioTrack) {
      $audioPath = (Resolve-Path -LiteralPath $AudioTrack).Path
      $mountArgs = @('--mount', "type=bind,source=$audioPath,target=/music/benchmark.wav,readonly")
    }
    docker run -d --cpus $CpuLimit --name $container --entrypoint /bin/bash @mountArgs -e SHORTS=0 -e "WIDTH=$Width" -e "HEIGHT=$Height" -e FPS=30 -e CAPTURE_FPS=30 -e "VIDEO_BITRATE=$VideoBitrate" -e "BUFSIZE=$BufferSize" -e PRESET=superfast -e OUTPUT=/recordings/bench.mp4 -e "RECORD_SECONDS=$EncodeSeconds" $Image -c $supervisor | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not start encode benchmark for $Image" }
    do {
      Start-Sleep -Seconds 1
      $state = docker inspect $container --format '{{.State.Running}}' 2>$null
    } while ($state -eq "true" -and $timer.Elapsed.TotalSeconds -lt 150)
    $timer.Stop()
    if ($state -eq "true") { throw "Encode benchmark exceeded 150 seconds for $Image" }
    $status = docker inspect $container --format '{{.State.ExitCode}}'
    if ($status -ne "0") {
      $logs = docker logs $container 2>&1 | Out-String
      throw "Encode benchmark exited $status for $Image`n$logs"
    }
    $tmp = Join-Path $env:TEMP "anyps5-encode-$Name.mp4"
    docker cp "${container}:/recordings/bench.mp4" $tmp | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "No MP4 output from $Image" }
    $cpuStat = Join-Path $env:TEMP "anyps5-cpu-$Name.stat"
    docker cp "${container}:/recordings/cpu.stat" $cpuStat | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "No cgroup CPU accounting from $Image" }
    $cpuLine = Get-Content -LiteralPath $cpuStat -TotalCount 1
    if ($cpuLine -notmatch '^\s*(\d+)\s*$') { throw "Could not parse cgroup CPU accounting from $Image" }
    $cpuMicros = [double]$Matches[1]
    $memoryPeakPath = Join-Path $env:TEMP "anyps5-memory-$Name.peak"
    docker cp "${container}:/recordings/memory.peak" $memoryPeakPath | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "No cgroup peak-memory accounting from $Image" }
    $memoryPeakBytes = [double](Get-Content -LiteralPath $memoryPeakPath -TotalCount 1)
    $file = Get-Item -LiteralPath $tmp
    $frameOutput = & ffprobe -v error -count_frames -select_streams v:0 -show_entries stream=nb_read_frames -of default=noprint_wrappers=1:nokey=1 $tmp
    if ($LASTEXITCODE -ne 0 -or $frameOutput -notmatch '^\s*(\d+)\s*$') { throw "Could not count encoded frames for $Image" }
    $frameCount = [int]$Matches[1]
    $result = [ordered]@{
      image = $Image
      outputSeconds = $EncodeSeconds
      wallSeconds = [Math]::Round($timer.Elapsed.TotalSeconds, 2)
      outputMiB = [Math]::Round($file.Length / 1MB, 2)
      cgroupCpuSeconds = [Math]::Round($cpuMicros / 1000000, 2)
      cgroupCpuPercentOfWall = if ($timer.Elapsed.TotalSeconds -gt 0) { [Math]::Round($cpuMicros / 1000000 / $timer.Elapsed.TotalSeconds * 100, 1) } else { $null }
      cgroupMemoryPeakMiB = [Math]::Round($memoryPeakBytes / 1MB, 1)
      videoFrames = $frameCount
      averageVideoFps = [Math]::Round($frameCount / $EncodeSeconds, 2)
    }
    Remove-Item -LiteralPath $tmp -Force
    Remove-Item -LiteralPath $cpuStat -Force
    Remove-Item -LiteralPath $memoryPeakPath -Force
    return $result
  } finally {
    docker rm -f $container 2>$null | Out-Null
  }
}

function Measure-Image([string]$Image, [string]$Name) {
  $container = "anyps5-bench-$Name"
  $port = $HostPort
  $client = [System.Net.Http.HttpClient]::new()
  $client.Timeout = [TimeSpan]::FromSeconds(30)
  try {
    $imageBytes = [double](docker image inspect $Image --format '{{.Size}}')
    if ($LASTEXITCODE -ne 0) { throw "Image not found: $Image" }
    $startup = [System.Diagnostics.Stopwatch]::StartNew()
    docker run -d --name $container -p "${port}:3000" $Image | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not start $Image" }
    $base = "http://127.0.0.1:$port"
    $ready = $false
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    while ([DateTime]::UtcNow -lt $deadline) {
      try {
        $response = $client.GetAsync("$base/healthz").GetAwaiter().GetResult()
        if ($response.IsSuccessStatusCode) { $ready = $true; break }
      } catch { }
      Start-Sleep -Milliseconds 200
    }
    $startup.Stop()
    if (-not $ready) { throw "$Image did not become healthy within 90 seconds" }
    $dataReady = $false
    $dataDeadline = [DateTime]::UtcNow.AddSeconds(60)
    while ([DateTime]::UtcNow -lt $dataDeadline) {
      try {
        $stateResponse = $client.GetAsync("$base/api/state").GetAwaiter().GetResult()
        $stateBytes = $stateResponse.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
        $stateJson = [System.Text.Encoding]::UTF8.GetString($stateBytes) | ConvertFrom-Json
        if ($stateResponse.IsSuccessStatusCode -and
            $null -ne $stateJson.progress.done -and
            $null -ne $stateJson.libraries.count -and
            $stateJson.sources.progress.ok -eq $true -and
            $stateJson.sources.history.ok -eq $true) {
          $dataReady = $true
          break
        }
      } catch { }
      Start-Sleep -Milliseconds 500
    }
    if (-not $dataReady) { throw "$Image did not collect core dashboard data within 60 seconds" }
    Start-Sleep -Seconds 10

    $latencies = [System.Collections.Generic.List[double]]::new()
    $responseBytes = 0
    for ($i = 0; $i -lt ($Samples + 5); $i++) {
      $timer = [System.Diagnostics.Stopwatch]::StartNew()
      $response = $client.GetAsync("$base/api/state").GetAwaiter().GetResult()
      $body = $response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult()
      $timer.Stop()
      if (-not $response.IsSuccessStatusCode) { throw "$Image /api/state returned $($response.StatusCode)" }
      if ($i -ge 5) { $latencies.Add($timer.Elapsed.TotalMilliseconds); $responseBytes = $body.Length }
    }

    $stats = docker stats --no-stream --format '{{.MemUsage}}' $container
    if ($LASTEXITCODE -ne 0) { throw "Could not read docker stats for $container" }
    $memory = ($stats -split '/')[0].Trim()
    $sorted = @($latencies | Sort-Object)
    return [ordered]@{
      image = $Image
      imageMiB = [Math]::Round($imageBytes / 1MB, 1)
      readyMs = [Math]::Round($startup.Elapsed.TotalMilliseconds, 0)
      apiStateSamples = $Samples
      apiStateBytes = $responseBytes
      apiStateMedianMs = Get-Percentile $latencies.ToArray() 0.50
      apiStateP95Ms = Get-Percentile $latencies.ToArray() 0.95
      idleMemory = $memory
    }
  } finally {
    $client.Dispose()
    docker rm -f $container 2>$null | Out-Null
  }
}

$results = @(
  (Measure-Image $BaselineImage "baseline")
  (Measure-Image $CandidateImage "candidate")
)
for ($run = 1; $run -le $EncodeRuns; $run++) {
  $results += Measure-Encode $BaselineImage "baseline-$run"
  $results += Measure-Encode $CandidateImage "candidate-$run"
}
$results | ConvertTo-Json -Depth 4
