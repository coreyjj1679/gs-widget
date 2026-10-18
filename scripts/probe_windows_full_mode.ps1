param(
    [Parameter(Mandatory = $true)][string]$BaselineZip,
    [Parameter(Mandatory = $true)][string]$TestRoot
)

# Diagnose startup geometry independently of updating. This runs the immutable
# published v1.0.0 executable with disposable preferences; no updater is invoked.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw "Full-mode probe requires PowerShell 7 on Windows" }
. (Join-Path $PSScriptRoot "windows_gui_test_helpers.ps1")

if (-not ("GersangFullModeProbe" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

[StructLayout(LayoutKind.Sequential)]
public struct GersangFullModeRect
{
    public int Left, Top, Right, Bottom;
}

[StructLayout(LayoutKind.Sequential)]
public struct GersangFullModeMonitor
{
    public uint Size;
    public GersangFullModeRect Monitor;
    public GersangFullModeRect Work;
    public uint Flags;
}

public static class GersangFullModeProbe
{
    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetClientRect(IntPtr window, out GersangFullModeRect rect);

    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetWindowRect(IntPtr window, out GersangFullModeRect rect);

    [DllImport("user32.dll", ExactSpelling = true)]
    public static extern uint GetDpiForWindow(IntPtr window);

    [DllImport("user32.dll", ExactSpelling = true)]
    public static extern IntPtr MonitorFromWindow(IntPtr window, uint flags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetMonitorInfoW(IntPtr monitor, ref GersangFullModeMonitor info);
}
'@
}

function Read-ProbeGeometry {
    param([uint32]$AppProcessId)
    $window = [GersangSmokeWindow]::Find($AppProcessId)
    if ($window -eq [IntPtr]::Zero) { throw "Cannot find the real app window for geometry capture" }
    $clientRect = [GersangFullModeRect]::new()
    $windowRect = [GersangFullModeRect]::new()
    if (-not [GersangFullModeProbe]::GetClientRect($window, [ref]$clientRect)) { throw "GetClientRect failed" }
    if (-not [GersangFullModeProbe]::GetWindowRect($window, [ref]$windowRect)) { throw "GetWindowRect failed" }
    $dpi = [GersangFullModeProbe]::GetDpiForWindow($window)
    if ($dpi -eq 0) { throw "GetDpiForWindow failed" }
    $monitor = [GersangFullModeMonitor]::new()
    $monitor.Size = [System.Runtime.InteropServices.Marshal]::SizeOf([type][GersangFullModeMonitor])
    $monitorHandle = [GersangFullModeProbe]::MonitorFromWindow($window, 2)
    if (-not [GersangFullModeProbe]::GetMonitorInfoW($monitorHandle, [ref]$monitor)) { throw "GetMonitorInfoW failed" }
    return [ordered]@{
        dpi = $dpi
        client = @{
            width = $clientRect.Right - $clientRect.Left
            height = $clientRect.Bottom - $clientRect.Top
        }
        logicalClient = @{
            width = ($clientRect.Right - $clientRect.Left) * 96.0 / $dpi
            height = ($clientRect.Bottom - $clientRect.Top) * 96.0 / $dpi
        }
        window = @{
            x = $windowRect.Left
            y = $windowRect.Top
            width = $windowRect.Right - $windowRect.Left
            height = $windowRect.Bottom - $windowRect.Top
        }
        monitorWorkArea = @{
            left = $monitor.Work.Left
            top = $monitor.Work.Top
            right = $monitor.Work.Right
            bottom = $monitor.Work.Bottom
        }
        responsive = [GersangSmokeWindow]::Responds($window)
    }
}

function Read-SavedMode {
    param([string]$WindowText)
    foreach ($line in ($WindowText -split "`n")) {
        if ($line.StartsWith("mode=")) { return $line.Substring(5).Trim() }
    }
    return "missing"
}

$archive = (Resolve-Path -LiteralPath $BaselineZip).Path
$root = [System.IO.Path]::GetFullPath($TestRoot)
if (Test-Path -LiteralPath $root) { throw "TestRoot must be a new disposable directory: $root" }
$archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($archiveHash -ne "5e036aad294b60cab04f4ca22271f28bedd5b47e281cced5a463561f88230098") {
    throw "Full-mode probe requires the exact published v1.0.0 ZIP"
}
New-Item -ItemType Directory -Path $root | Out-Null
$install = Join-Path $root "released app with spaces"
Expand-Archive -LiteralPath $archive -DestinationPath $install
$executable = Join-Path $install "gersang-widget.exe"
$resource = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($executable)
$cases = [System.Collections.Generic.List[object]]::new()
$report = [ordered]@{
    status = "running"
    baselineArchiveSha256 = $archiveHash
    baselineProductVersion = $resource.ProductVersion
    baselineFileVersion = $resource.FileVersion
    scope = "Published v1.0.0 normal startup only; no update/check-installation helper invoked"
    cases = $cases
}
$overrides = @{
    LOCALAPPDATA = $null
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
$sizes = @(
    [pscustomobject]@{ Width = 960; Height = 640 },
    [pscustomobject]@{ Width = 1000; Height = 660 },
    [pscustomobject]@{ Width = 1120; Height = 720 }
)
$utf8 = [System.Text.UTF8Encoding]::new($false)
try {
    foreach ($size in $sizes) {
        for ($iteration = 1; $iteration -le 2; $iteration++) {
            $caseName = "$($size.Width)x$($size.Height)-run-$iteration"
            $caseDirectory = Join-Path $root $caseName
            $profileRoot = Join-Path $caseDirectory "isolated profile"
            $profileDirectory = Join-Path $profileRoot "gs-widget"
            New-Item -ItemType Directory -Path $profileDirectory -Force | Out-Null
            $env:LOCALAPPDATA = $profileRoot
            $windowText = "size=560,740`npos=32,32`nmode=full`nfull-pos=32,32`nfull-size=$($size.Width),$($size.Height)`n"
            # Match the non-default preferences of the original failed fixture.
            $preferences = @{
                "window.txt" = $windowText
                "favourites.txt" = "D01`nD02`n"
                "aliases.txt" = "# Dummy upgrade: Unicode aliases must survive.`n升級測試 1`nmy_dummy_alias 2`n"
                "ui-lang.txt" = "en-zh"
                "view.txt" = "pin=off`nbattle=0`ngallery=1`nanimate=0`nhotkeys=0`nlegacy=0`n"
                "dock.txt" = "v=1`nshow=110111111`norder=8,7,6,5,4,3,2,1,0`nsize=84,580`n"
                "home.txt" = "size=2`norder=8,7,6,5,4,3,2,1,0`n"
                "opacity.txt" = "85"
                "set-extra.txt" = "1"
                "window-macos.txt" = "preserve cross-platform personal settings`n"
                "client.txt" = (Join-Path $install "data/client")
            }
            foreach ($entry in $preferences.GetEnumerator()) {
                [System.IO.File]::WriteAllText((Join-Path $profileDirectory $entry.Key), $entry.Value, $utf8)
            }
            $windowPath = Join-Path $profileDirectory "window.txt"
            $samples = [System.Collections.Generic.List[object]]::new()
            $caseResult = [ordered]@{
                name = $caseName
                requestedFullWidth = $size.Width
                requestedFullHeight = $size.Height
                initialWindowText = $windowText
                samples = $samples
                finalWindowText = $null
                finalSavedMode = $null
                modePreserved = $false
                closedNormally = $false
                passed = $false
            }
            $cases.Add($caseResult)
            $appProcess = $null
            try {
                $started = [DateTime]::UtcNow
                $appProcess = Start-Process -FilePath $executable -WorkingDirectory $root -PassThru -RedirectStandardOutput (Join-Path $caseDirectory "stdout.log") -RedirectStandardError (Join-Path $caseDirectory "stderr.log")
                $caseResult["processId"] = $appProcess.Id
                Wait-Gui -AppProcess $appProcess
                $deadline = [DateTime]::UtcNow.AddSeconds(3)
                while ([DateTime]::UtcNow -lt $deadline) {
                    $appProcess.Refresh()
                    if ($appProcess.HasExited) { throw "App exited during full-mode observation" }
                    $observedText = [System.IO.File]::ReadAllText($windowPath)
                    $geometry = Read-ProbeGeometry -AppProcessId ([uint32]$appProcess.Id)
                    $samples.Add([ordered]@{
                        elapsedMilliseconds = [Math]::Round(([DateTime]::UtcNow - $started).TotalMilliseconds)
                        windowText = $observedText
                        savedMode = (Read-SavedMode -WindowText $observedText)
                        geometry = $geometry
                    })
                    if (-not $geometry.responsive) { throw "App stopped responding during full-mode observation" }
                    Start-Sleep -Milliseconds 250
                }
                Close-Gui -AppProcess $appProcess
                $caseResult["closedNormally"] = $true
                $caseResult["finalWindowText"] = [System.IO.File]::ReadAllText($windowPath)
                $caseResult["finalSavedMode"] = Read-SavedMode -WindowText $caseResult.finalWindowText
                $changedSamples = @($samples | Where-Object { $_.savedMode -ne "full" })
                $caseResult["modePreserved"] = $caseResult.finalSavedMode -eq "full" -and $changedSamples.Count -eq 0
                $caseResult["passed"] = $caseResult.modePreserved
                Write-Output "$caseName final saved mode: $($caseResult.finalSavedMode); preserved throughout observations: $($caseResult.modePreserved)"
                Write-Output ($caseResult.finalWindowText | ConvertTo-Json -Compress)
            }
            catch {
                $caseResult["error"] = $_.Exception.Message
                if (Test-Path -LiteralPath $windowPath) {
                    $caseResult["finalWindowText"] = [System.IO.File]::ReadAllText($windowPath)
                    $caseResult["finalSavedMode"] = Read-SavedMode -WindowText $caseResult.finalWindowText
                }
                Write-Warning "$caseName failed: $($_.Exception.Message)"
            }
            finally {
                if ($null -ne $appProcess) {
                    try {
                        $appProcess.Refresh()
                        if (-not $appProcess.HasExited) {
                            # Prefer normal close; kill only this probe's process
                            # if its GUI cannot answer the bounded close request.
                            try { Close-Gui -AppProcess $appProcess }
                            catch {
                                $appProcess.Refresh()
                                if (-not $appProcess.HasExited) { $appProcess.Kill() }
                                $null = $appProcess.WaitForExit(5000)
                            }
                        }
                    }
                    finally { $appProcess.Dispose() }
                }
                $caseResult | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath (Join-Path $caseDirectory "observations.json") -Encoding utf8
            }
        }
    }
    $failed = @($cases | Where-Object { -not $_.passed })
    $report["status"] = if ($failed.Count -eq 0) { "passed" } else { "failed" }
    $report["failedCases"] = @($failed | ForEach-Object { $_.name })
}
finally {
    foreach ($name in $previous.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previous[$name], "Process")
    }
    $report["finishedAtUtc"] = [DateTime]::UtcNow.ToString("o")
    $report | ConvertTo-Json -Depth 14 | Set-Content -LiteralPath (Join-Path $root "full-mode-probe.json") -Encoding utf8
}
if ($report.status -ne "passed") {
    throw "Published v1.0.0 full-mode startup probe failed; inspect full-mode-probe.json for saved preferences and actual native geometry."
}
Write-Output "Published v1.0.0 preserved full mode for all six isolated startup probes."
