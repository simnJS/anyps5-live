param(
  [string]$Output = "benchmark-tone.wav",
  [ValidateRange(1, 600)][int]$Seconds = 45
)

$ErrorActionPreference = "Stop"
$sampleRate = 44100
$channels = 2
$bitsPerSample = 16
$blockAlign = $channels * ($bitsPerSample / 8)
$dataBytes = $Seconds * $sampleRate * $blockAlign
$fullPath = [System.IO.Path]::GetFullPath($Output)
$directory = [System.IO.Path]::GetDirectoryName($fullPath)
if ($directory) { [System.IO.Directory]::CreateDirectory($directory) | Out-Null }

$source = @'
using System;
using System.IO;
using System.Text;

public static class BenchmarkToneWriter
{
    public static void Write(string path, int seconds)
    {
        const int sampleRate = 44100;
        const short channels = 2;
        const short blockAlign = 4;
        int frames = seconds * sampleRate;
        int dataBytes = frames * blockAlign;
        using var stream = File.Create(path);
        using var writer = new BinaryWriter(stream, Encoding.ASCII, leaveOpen: false);
        writer.Write(Encoding.ASCII.GetBytes("RIFF"));
        writer.Write(36 + dataBytes);
        writer.Write(Encoding.ASCII.GetBytes("WAVE"));
        writer.Write(Encoding.ASCII.GetBytes("fmt "));
        writer.Write(16);
        writer.Write((short)1);
        writer.Write(channels);
        writer.Write(sampleRate);
        writer.Write(sampleRate * blockAlign);
        writer.Write(blockAlign);
        writer.Write((short)16);
        writer.Write(Encoding.ASCII.GetBytes("data"));
        writer.Write(dataBytes);
        for (int frame = 0; frame < frames; frame++)
        {
            short sample = (short)Math.Round(3276.7 * Math.Sin(2 * Math.PI * 440 * frame / sampleRate));
            writer.Write(sample);
            writer.Write(sample);
        }
    }
}
'@
Add-Type -TypeDefinition $source -Language CSharp
[BenchmarkToneWriter]::Write($fullPath, $Seconds)

Write-Output "Created $fullPath ($Seconds seconds, 44.1 kHz stereo PCM)"
