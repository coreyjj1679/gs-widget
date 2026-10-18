param(
    [Parameter(Mandatory = $true)][string]$ExePath,
    [Parameter(Mandatory = $true)][string]$LocalAppData,
    [string]$WorkingDirectory = [System.IO.Path]::GetTempPath()
)

$ErrorActionPreference = "Stop"
if (-not $IsWindows) { throw "This GUI smoke check requires PowerShell 7 on Windows" }
$executable = (Resolve-Path -LiteralPath $ExePath).Path
$working = (Resolve-Path -LiteralPath $WorkingDirectory).Path
$smokeProfile = [System.IO.Path]::GetFullPath($LocalAppData)
New-Item -ItemType Directory -Force $smokeProfile | Out-Null
$stdout = Join-Path $smokeProfile "offline-smoke-stdout.txt"
$stderr = Join-Path $smokeProfile "offline-smoke-stderr.txt"
$aliases = Join-Path $smokeProfile "gs-widget/aliases.txt"

# Keep the local caller's environment unchanged. In Actions these values also
# remain confined to this step; artifact upload still has network access.
$overrides = @{
    LOCALAPPDATA = $smokeProfile
    GERSANG_DB = $null
    GERSANG_CLIENT = $null
    HTTPS_PROXY = "http://127.0.0.1:1"
    HTTP_PROXY = "http://127.0.0.1:1"
    ALL_PROXY = "http://127.0.0.1:1"
    NO_PROXY = ""
    SLINT_BACKEND = "winit-software"
    SLINT_RENDERER = "software"
}
$previous = @{}
foreach ($name in $overrides.Keys) {
    $previous[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
    [Environment]::SetEnvironmentVariable($name, $overrides[$name], "Process")
}

$process = $null
try {
    # No --check-installation argument: this exercises the ordinary app startup,
    # the background update check, the GUI event loop and profile initialization.
    $process = Start-Process -FilePath $executable -WorkingDirectory $working -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    $windowReady = $false
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh() # MainWindowHandle is otherwise cached by .NET.
        if ($process.HasExited) { throw "Offline app exited during startup: $($process.ExitCode)" }
        if ($process.MainWindowHandle -ne [IntPtr]::Zero -and (Test-Path -LiteralPath $aliases -PathType Leaf)) {
            $windowReady = $true
            break
        }
        Start-Sleep -Milliseconds 200
    }
    if (-not $windowReady) { throw "Offline app did not create its window and alias profile within 30 seconds" }

    # Observe beyond the update check's 30-second timeout, including a slow
    # failure path, and verify the window still processes Windows messages.
    $deadline = [DateTime]::UtcNow.AddSeconds(35)
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) { throw "App exited after the offline background check: $($process.ExitCode)" }
        if ($process.MainWindowHandle -eq [IntPtr]::Zero) { throw "App lost its main window after the offline background check" }
        if (-not $process.Responding) { throw "App stopped responding during the offline background check" }
        Start-Sleep -Milliseconds 200
    }

    if (-not $process.CloseMainWindow()) { throw "Could not ask the offline app to close normally" }
    if (-not $process.WaitForExit(10000)) { throw "Offline app did not close normally within 10 seconds" }
    if ($process.ExitCode -ne 0) { throw "Offline app closed with error: $($process.ExitCode)" }
    Write-Output "Offline GUI smoke passed: window stayed responsive for 35 seconds, aliases profile initialized, and normal close succeeded with a refused network proxy."
}
finally {
    try {
        if ($null -ne $process) {
            $process.Refresh()
            if (-not $process.HasExited) {
                try { $process.Kill() }
                catch {
                    # A normal exit can race cleanup between HasExited and Kill.
                    if (-not $process.HasExited) { throw }
                }
                $null = $process.WaitForExit(5000)
            }
        }
    }
    finally {
        if ($null -ne $process) { $process.Dispose() }
        foreach ($name in $previous.Keys) {
            [Environment]::SetEnvironmentVariable($name, $previous[$name], "Process")
        }
        foreach ($log in @($stdout, $stderr)) {
            if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log }
        }
    }
}
