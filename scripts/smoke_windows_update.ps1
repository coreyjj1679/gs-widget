param(
    [Parameter(Mandatory = $true)][string]$BaselineZip,
    [Parameter(Mandatory = $true)][string]$CandidateZip,
    [Parameter(Mandatory = $true)][string]$TestRoot,
    [string]$BaselineVersion = "1.0.0",
    [string]$CandidateVersion = "1.0.1"
)

# This is a disposable integration test, not an alternate updater channel.
# The published old executable supplies the helper; the new ZIP stays local.
# Discovery, HTTP download and pressing the GUI update button are outside this
# test's scope. The production helper, process handoff and GUI restart are real.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw "Dummy update smoke requires PowerShell 7 on Windows" }
$baselineArchive = (Resolve-Path -LiteralPath $BaselineZip).Path
$candidateArchive = (Resolve-Path -LiteralPath $CandidateZip).Path
$root = [System.IO.Path]::GetFullPath($TestRoot)
$fixture = Join-Path $PSScriptRoot "windows_update_fixture.py"
if (Test-Path -LiteralPath $root) { throw "TestRoot must be a new disposable directory: $root" }

function Invoke-Fixture {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & python $fixture @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Fixture verification failed: $($Arguments -join ' ')" }
}

Invoke-Fixture prepare --root $root --baseline-zip $baselineArchive --candidate-zip $candidateArchive --candidate-version $CandidateVersion
$install = Join-Path $root "app with spaces"
$executable = Join-Path $install "gersang-widget.exe"
$candidateExe = Join-Path $root "candidate package/gersang-widget.exe"
$logs = Join-Path $root "logs"
$processes = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$checks = [System.Collections.Generic.List[string]]::new()
$report = [ordered]@{
    status = "running"
    baselineVersion = $BaselineVersion
    candidateVersion = $CandidateVersion
    testRoot = $root
    scope = "Published baseline helper, local staged payload, actual old GUI shutdown and new GUI restart; no release publication or GUI discovery/download automation"
    checks = $checks
}

function Start-SmokeProcess {
    param([string]$FilePath, [string[]]$Arguments = @(), [string]$Name)
    # ArgumentList in Start-Process joins its elements. Quote individual paths
    # so this test deliberately covers installation/plan locations with spaces.
    $start = @{
        FilePath = $FilePath
        WorkingDirectory = $root
        PassThru = $true
        RedirectStandardOutput = (Join-Path $logs "$Name-stdout.log")
        RedirectStandardError = (Join-Path $logs "$Name-stderr.log")
    }
    if ($Arguments.Count -gt 0) {
        $start.ArgumentList = @($Arguments | ForEach-Object { '"' + $_ + '"' })
    }
    $child = Start-Process @start
    $processes.Add($child)
    return $child
}

function Assert-Version {
    param([string]$FilePath, [string]$Expected)
    $resource = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($FilePath)
    foreach ($value in @($resource.FileVersion, $resource.ProductVersion)) {
        if ($value -ne $Expected -and $value -ne "$Expected.0") {
            throw "Unexpected executable version '$value', expected $Expected in $FilePath"
        }
    }
    return @{ fileVersion = $resource.FileVersion; productVersion = $resource.ProductVersion }
}

function Assert-Installation {
    param([string]$FilePath, [string]$Expected, [string]$Name)
    $child = Start-SmokeProcess -FilePath $FilePath -Arguments @("--check-installation") -Name $Name
    if (-not $child.WaitForExit(30000)) { throw "Installation check timed out: $Name" }
    if ($child.ExitCode -ne 0) { throw "Installation check failed: $Name, exit $($child.ExitCode)" }
    $output = Get-Content -LiteralPath (Join-Path $logs "$Name-stdout.log") -Raw
    if (-not $output.Contains("Gersang ${Expected}: installation OK")) {
        throw "Installation check did not identify expected version ${Expected}: $output"
    }
}

function Wait-Gui {
    param([System.Diagnostics.Process]$AppProcess, [int]$Seconds = 30)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $AppProcess.Refresh()
        if ($AppProcess.HasExited) { throw "GUI exited during startup: $($AppProcess.Id), exit $($AppProcess.ExitCode)" }
        if ($AppProcess.MainWindowHandle -ne [IntPtr]::Zero -and $AppProcess.Responding) { return }
        Start-Sleep -Milliseconds 200
    }
    throw "GUI did not become responsive within $Seconds seconds"
}

function Assert-GuiResponsive {
    param([System.Diagnostics.Process]$AppProcess, [int]$Seconds)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $AppProcess.Refresh()
        if ($AppProcess.HasExited -or $AppProcess.MainWindowHandle -eq [IntPtr]::Zero -or -not $AppProcess.Responding) {
            throw "GUI stopped responding during observation: $($AppProcess.Id)"
        }
        Start-Sleep -Milliseconds 200
    }
}

function Close-Gui {
    param([System.Diagnostics.Process]$AppProcess)
    $AppProcess.Refresh()
    if (-not $AppProcess.CloseMainWindow()) { throw "Could not request normal GUI shutdown: $($AppProcess.Id)" }
    if (-not $AppProcess.WaitForExit(10000)) { throw "GUI did not close normally within 10 seconds" }
    if ($AppProcess.ExitCode -ne 0) { throw "GUI closed with error: $($AppProcess.ExitCode)" }
}

function Wait-Marker {
    param([string]$Directory, [string]$Marker, [System.Diagnostics.Process]$Helper, [int]$Seconds = 30)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $errorPath = Join-Path $Directory "error.txt"
        if ($Marker -ne "error.txt" -and (Test-Path -LiteralPath $errorPath)) {
            throw "Update helper failed: $(Get-Content -LiteralPath $errorPath -Raw)"
        }
        if (Test-Path -LiteralPath (Join-Path $Directory $Marker)) { return }
        $Helper.Refresh()
        if ($Helper.HasExited) { throw "Helper exited before $Marker appeared, exit $($Helper.ExitCode)" }
        Start-Sleep -Milliseconds 100
    }
    throw "Timed out waiting for updater $Marker marker"
}

$overrides = @{
    LOCALAPPDATA = (Join-Path $root "isolated profile")
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

try {
    $report["baselineResource"] = Assert-Version -FilePath $executable -Expected $BaselineVersion
    $report["candidateResource"] = Assert-Version -FilePath $candidateExe -Expected $CandidateVersion
    Assert-Installation -FilePath $executable -Expected $BaselineVersion -Name "baseline-installation"
    Assert-Installation -FilePath $candidateExe -Expected $CandidateVersion -Name "candidate-installation"
    $checks.Add("Both real Windows executables report the expected embedded/resource versions and pass installation checks")

    # Normalize legitimate geometry saves before recording byte-exact evidence.
    $warmup = Start-SmokeProcess -FilePath $executable -Name "baseline-warmup"
    Wait-Gui -AppProcess $warmup
    Assert-GuiResponsive -AppProcess $warmup -Seconds 3
    Close-Gui -AppProcess $warmup
    Invoke-Fixture snapshot --root $root
    $checks.Add("Published baseline GUI reads seeded non-default profile and closes normally")

    $oldGui = Start-SmokeProcess -FilePath $executable -Name "baseline-update-parent"
    Wait-Gui -AppProcess $oldGui
    Assert-GuiResponsive -AppProcess $oldGui -Seconds 3
    $report["oldGuiPid"] = $oldGui.Id

    # Reject a modified staged file while the real baseline GUI remains open.
    Invoke-Fixture stage --root $root --parent-pid $oldGui.Id --case tampered
    $badStage = Join-Path $install ".gs-widget-update-smoke-tampered"
    $badHelper = Start-SmokeProcess -FilePath (Join-Path $badStage "updater.exe") -Arguments @("--apply-update", (Join-Path $badStage "plan.json")) -Name "tampered-helper"
    Wait-Marker -Directory $badStage -Marker "error.txt" -Helper $badHelper
    Invoke-Fixture verify --root $root --phase rejected
    Assert-GuiResponsive -AppProcess $oldGui -Seconds 1
    # An error uses a modal MessageBox. Its marker has already proved rejection;
    # stop this failed disposable helper so the CI desktop is not left blocked.
    $badHelper.Refresh()
    if (-not $badHelper.HasExited) { $badHelper.Kill() }
    if (-not $badHelper.WaitForExit(5000)) { throw "Failed helper did not terminate" }
    $checks.Add("Tampered payload rejected before ready; baseline GUI, application and all personal data remain intact")

    Invoke-Fixture stage --root $root --parent-pid $oldGui.Id --case success
    $stage = Join-Path $install ".gs-widget-update-smoke-success"
    $helper = Start-SmokeProcess -FilePath (Join-Path $stage "updater.exe") -Arguments @("--apply-update", (Join-Path $stage "plan.json")) -Name "update-helper"
    Wait-Marker -Directory $stage -Marker "ready" -Helper $helper
    $oldGui.Refresh()
    if ($oldGui.HasExited) { throw "Old GUI exited before helper acknowledged readiness" }
    if (Test-Path -LiteralPath (Join-Path $stage "backup")) { throw "Helper modified installation while old GUI was running" }
    Close-Gui -AppProcess $oldGui
    Wait-Marker -Directory $stage -Marker "completed" -Helper $helper
    if (-not $helper.WaitForExit(10000)) { throw "Successful update helper did not exit" }
    if ($helper.ExitCode -ne 0) { throw "Successful update helper exit code was $($helper.ExitCode)" }
    $checks.Add("Published v1.0.0 helper acknowledges readiness, waits for actual old GUI exit and completes replacement")

    $newGui = $null
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while ([DateTime]::UtcNow -lt $deadline) {
        $matches = @(Get-Process -Name "gersang-widget" -ErrorAction SilentlyContinue | Where-Object {
            $_.Id -ne $oldGui.Id -and $_.Path -eq $executable
        })
        if ($matches.Count -gt 1) { throw "Updater unexpectedly started multiple GUI processes" }
        if ($matches.Count -eq 1) { $newGui = $matches[0]; break }
        Start-Sleep -Milliseconds 200
    }
    if ($null -eq $newGui) { throw "Updater did not restart the installed application" }
    $processes.Add($newGui)
    $report["newGuiPid"] = $newGui.Id
    $report["installedResource"] = Assert-Version -FilePath $executable -Expected $CandidateVersion
    Wait-Gui -AppProcess $newGui
    Assert-GuiResponsive -AppProcess $newGui -Seconds 35
    Close-Gui -AppProcess $newGui
    Invoke-Fixture verify --root $root --phase updated
    Assert-Installation -FilePath $executable -Expected $CandidateVersion -Name "updated-installation"
    $checks.Add("New v1.0.1 GUI restarts from the same installation, stays responsive offline for 35 seconds and closes normally")
    $checks.Add("All 11 personal files and 13 unrelated/legacy files survive byte-for-byte; new package and original recovery backup hashes match")
    $report["status"] = "passed"
    Write-Output "Dummy Windows update passed: $BaselineVersion -> $CandidateVersion, real helper/restart, tamper rejection, offline GUI, and personal-data preservation."
}
catch {
    $report["status"] = "failed"
    $report["error"] = $_.Exception.Message
    throw
}
finally {
    # Only kill test-owned processes. Include a just-restarted child if failure
    # occurred before process discovery added it to the tracked list.
    foreach ($child in @(Get-Process -Name "gersang-widget", "updater" -ErrorAction SilentlyContinue)) {
        try {
            if ($child.Path -and $child.Path.StartsWith($root + [System.IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                $child.Kill()
                $null = $child.WaitForExit(5000)
            }
        }
        catch { Write-Warning "Could not clean up a disposable process: $($_.Exception.Message)" }
        finally { $child.Dispose() }
    }
    foreach ($child in $processes) { $child.Dispose() }
    foreach ($name in $previous.Keys) {
        [Environment]::SetEnvironmentVariable($name, $previous[$name], "Process")
    }
    $report["finishedAtUtc"] = [DateTime]::UtcNow.ToString("o")
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $root "smoke-report.json") -Encoding utf8
    foreach ($log in @(Get-ChildItem -LiteralPath $logs -Filter "*.log")) {
        if ($log.Length -gt 0) { Write-Output "$($log.Name):"; Get-Content -LiteralPath $log.FullName }
    }
}
