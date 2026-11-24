param([Parameter(Mandatory)][string]$InstallDirectory)

# Run against the installed binaries on an isolated Windows CI desktop.
# This catches a window that appears and exits during graphics initialization.
$ErrorActionPreference = 'Stop'
$binary = Join-Path $InstallDirectory 'upyr-settings.exe'
$previousConfig = $env:UPYR_CONFIG
$previousLog = $env:RUST_LOG
$root = Join-Path ([IO.Path]::GetTempPath()) ('upyr-settings-test-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path $root | Out-Null
try {
    $env:RUST_LOG = 'warn,upyr=info,egui_wgpu=debug'
    foreach ($mode in @('pixel', 'native')) {
        $env:UPYR_CONFIG = Join-Path $root "$mode.toml"
        $stderr = Join-Path $root "$mode.stderr.txt"
        $stdout = Join-Path $root "$mode.stdout.txt"
        $start = @{
            FilePath = $binary
            PassThru = $true
            RedirectStandardError = $stderr
            RedirectStandardOutput = $stdout
        }
        if ($mode -eq 'pixel') { $start.ArgumentList = '--pixel' }
        $process = Start-Process @start
        try {
            $deadline = [DateTime]::UtcNow.AddSeconds(30)
            do {
                Start-Sleep -Milliseconds 250
                $process.Refresh()
                if ($process.HasExited) { throw "$mode settings exited during startup: $($process.ExitCode)" }
            } while ($process.MainWindowHandle -eq 0 -and [DateTime]::UtcNow -lt $deadline)
            if ($process.MainWindowHandle -eq 0) { throw "$mode settings did not open a window" }
            Start-Sleep -Seconds 10
            $process.Refresh()
            if ($process.HasExited -or -not $process.Responding) {
                throw "$mode settings closed or stopped responding"
            }
            $renderLog = Get-Content $stderr -Raw
            if ($renderLog -notmatch 'Picked.*wgpu adapter:.*backend: Dx12') {
                throw "$mode settings did not initialize the DX12 renderer"
            }
            if (-not $process.CloseMainWindow()) { throw "$mode settings refused close" }
            if (-not $process.WaitForExit(10000)) { throw "$mode settings did not exit cleanly" }
            if ($process.ExitCode -ne 0) { throw "$mode settings failed: $($process.ExitCode)" }
            Write-Output "$mode settings: window opened, DX12 initialized, responsive for 10 seconds, clean exit."
        } finally {
            if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
            if (Test-Path $stderr) { Get-Content $stderr }
        }
    }
} finally {
    $env:UPYR_CONFIG = $previousConfig
    $env:RUST_LOG = $previousLog
}
