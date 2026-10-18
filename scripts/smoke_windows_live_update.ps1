param(
    [Parameter(Mandatory = $true)][string]$BaselineZip,
    [Parameter(Mandatory = $true)][string]$TestRoot,
    [ValidatePattern('^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$')][string]$TargetVersion = "1.0.1",
    [ValidateRange(1, 120)][int]$WaitForReleaseMinutes = 40
)

# Exercise the public updater exclusively through observed UI Automation buttons.
# Only the app downloads, stages, starts its helper, closes itself and installs.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw "Live update smoke requires PowerShell 7 on Windows" }
. (Join-Path $PSScriptRoot "windows_gui_test_helpers.ps1")
$fixture = Join-Path $PSScriptRoot "windows_live_update_fixture.py"
$uiScript = Join-Path $PSScriptRoot "windows_update_ui.ps1"
$root = [System.IO.Path]::GetFullPath($TestRoot)
$archive = (Resolve-Path -LiteralPath $BaselineZip).Path
if (Test-Path -LiteralPath $root) { throw "TestRoot must be a new disposable directory" }

function Invoke-LiveFixture {
    param([string[]]$Arguments)
    & python $fixture @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Live update fixture failed: $($Arguments -join ' ')" }
}
Invoke-LiveFixture -Arguments @("prepare", "--root", $root, "--baseline-zip", $archive)
$install = Join-Path $root "app with spaces"
$executable = Join-Path $install "gersang-widget.exe"
$logs = Join-Path $root "logs"
$evidence = Join-Path $root "ui evidence"
New-Item -ItemType Directory -Path $evidence | Out-Null
$tracked = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
$checks = [System.Collections.Generic.List[string]]::new()
$uiStates = [System.Collections.Generic.List[object]]::new()
$report = [ordered]@{
    status = "running"
    baselineVersion = "1.0.0"
    targetVersion = $TargetVersion
    scope = "Published baseline GUI uses its production GitHub endpoint, Download update and Restart and update actions"
    manualCheckInvoked = $false
    manualCheckCount = 0
    backgroundCheckFoundTarget = $false
    checks = $checks
    uiStates = $uiStates
}

# This optional credential belongs only to the independent release observer.
# Clear it before starting ANY app/helper/UI worker; production app stays public
# and unauthenticated, exactly as downloaded by an end user.
$observerToken = [Environment]::GetEnvironmentVariable("CI_RELEASE_TOKEN", "Process")
$overrides = @{
    LOCALAPPDATA = (Join-Path $root "isolated profile")
    GERSANG_DB = $null
    GERSANG_CLIENT = $null
    HTTPS_PROXY = $null
    HTTP_PROXY = $null
    ALL_PROXY = $null
    NO_PROXY = $null
    CI_RELEASE_TOKEN = $null
    SLINT_BACKEND = "winit-software"
    SLINT_RENDERER = "software"
}
$previous = @{}
foreach ($name in $overrides.Keys) {
    $previous[$name] = [Environment]::GetEnvironmentVariable($name, "Process")
    [Environment]::SetEnvironmentVariable($name, $overrides[$name], "Process")
}

function Write-LiveJson {
    param([string]$Path, $Value)
    $Value | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $Path -Encoding utf8
}

function Start-LiveGui {
    param([string]$Name)
    $child = Start-Process -FilePath $executable -WorkingDirectory $root -PassThru -RedirectStandardOutput (Join-Path $logs "$Name-stdout.log") -RedirectStandardError (Join-Path $logs "$Name-stderr.log")
    # Retain a handle while it is alive so its true exit status remains queryable.
    $null = $child.Handle
    $tracked.Add($child)
    Wait-Gui -AppProcess $child
    return $child
}

function Read-LiveUi {
    param([System.Diagnostics.Process]$AppProcess, [string]$State)
    $directory = Join-Path $evidence $State
    $raw = & $uiScript -Mode Inspect -ProcessId $AppProcess.Id -OutputDirectory $directory | Out-String
    $summary = $raw | ConvertFrom-Json
    $tree = Get-Content -LiteralPath $summary.json -Raw -Encoding utf8 | ConvertFrom-Json
    $uiStates.Add(@{ state = $State; processId = $AppProcess.Id; json = $summary.json; capturedAtUtc = [DateTime]::UtcNow.ToString("o") })
    return $tree
}

function Find-LiveButton {
    param($Tree, [string[]]$Names)
    $found = @($Tree.nodes | Where-Object {
        $_.PSObject.Properties.Name -contains "name" -and
        $_.controlType -eq "ControlType.Button" -and $_.enabled -and -not $_.offscreen -and
        $Names -ccontains $_.name
    })
    if ($found.Count -gt 1) { throw "Observed more than one enabled button for: $($Names -join ', ')" }
    if ($found.Count -eq 1) { return $found[0] }
    return $null
}

function Wait-LiveButton {
    param([System.Diagnostics.Process]$AppProcess, [string[]]$Names, [string]$State, [int]$Seconds = 90)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $AppProcess.Refresh()
        if ($AppProcess.HasExited) { throw "App exited while waiting for $State" }
        $tree = Read-LiveUi -AppProcess $AppProcess -State $State
        $button = Find-LiveButton -Tree $tree -Names $Names
        if ($null -ne $button) { return @{ Tree = $tree; Button = $button } }
        $retry = Find-LiveButton -Tree $tree -Names @("Try again")
        if ($null -ne $retry -and $Names -cnotcontains "Try again") {
            throw "App reported an update error while waiting for $State; inspect captured UI status"
        }
        Start-Sleep -Seconds 2
    }
    throw "Timed out waiting for observed button(s) $($Names -join ', ') during $State"
}

function Invoke-LiveButton {
    param([System.Diagnostics.Process]$AppProcess, $Button, [string]$State)
    if ($null -eq $Button) { throw "An observed button is required for $State" }
    $raw = & $uiScript -Mode Act -ProcessId $AppProcess.Id -OutputDirectory (Join-Path $evidence $State) -ButtonName $Button.name | Out-String
    $action = $raw | ConvertFrom-Json
    $uiStates.Add(@{ state = $State; processId = $AppProcess.Id; actionJson = $action.actionJson; observedName = $Button.name })
}

function Open-LiveAppSettings {
    param([System.Diagnostics.Process]$AppProcess, [string]$State)
    $settings = Wait-LiveButton -AppProcess $AppProcess -Names @("設定 / Settings", "Settings") -State "$State-settings-button"
    Invoke-LiveButton -AppProcess $AppProcess -Button $settings.Button -State "$State-open-settings"
    $appTab = Wait-LiveButton -AppProcess $AppProcess -Names @("App") -State "$State-app-tab"
    Invoke-LiveButton -AppProcess $AppProcess -Button $appTab.Button -State "$State-open-app-tab"
    return (Read-LiveUi -AppProcess $AppProcess -State "$State-app-settings")
}

function Assert-LiveVersion {
    param([string]$Expected)
    $resource = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($executable)
    foreach ($value in @($resource.ProductVersion, $resource.FileVersion)) {
        if ($value -ne $Expected -and $value -ne "$Expected.0") { throw "Installed executable reports '$value', expected $Expected" }
    }
    return @{ productVersion = $resource.ProductVersion; fileVersion = $resource.FileVersion }
}

function Wait-PublicRelease {
    param([System.Diagnostics.Process]$AppProcess)
    $headers = @{ Accept = "application/vnd.github+json"; "X-GitHub-Api-Version" = "2022-11-28"; "User-Agent" = "gs-widget-public-update-smoke" }
    if ($observerToken) { $headers.Authorization = "Bearer $observerToken" }
    $deadline = [DateTime]::UtcNow.AddMinutes($WaitForReleaseMinutes)
    $name = "gs-widget-v$TargetVersion-windows-x64.zip"
    $url = "https://github.com/coreyjj1679/gs-widget/releases/download/v$TargetVersion/$name"
    while ([DateTime]::UtcNow -lt $deadline) {
        Assert-GuiResponsive -AppProcess $AppProcess -Seconds 1
        $release = $null
        try {
            $release = Invoke-RestMethod -Uri "https://api.github.com/repos/coreyjj1679/gs-widget/releases/latest" -Headers $headers -TimeoutSec 30
        }
        catch { $report["lastObserverError"] = $_.Exception.Message }
        if ($null -ne $release -and $release.tag_name -eq "v$TargetVersion") {
            if ($release.draft -or $release.prerelease -or -not $release.published_at) { throw "Target must be a published stable Release" }
            if (-not [string]::IsNullOrEmpty([string]$release.body)) { throw "Published target Release unexpectedly contains patch notes" }
            $assets = @($release.assets | Where-Object { $_.name -eq $name -and $_.browser_download_url -eq $url })
            if ($assets.Count -ne 1) { throw "Published target must have exactly one expected Windows ZIP" }
            $asset = $assets[0]
            if ($asset.size -le 0) { throw "Published target ZIP has an invalid size" }
            if ([string]$asset.digest -match '^sha256:[0-9a-fA-F]{64}$') {
                Write-LiveJson -Path (Join-Path $root "observed-public-release.json") -Value $release
                return @{ Version = $TargetVersion; Url = $url; Size = [long]$asset.size; Sha256 = ([string]$asset.digest).Substring(7).ToLowerInvariant(); PublishedAt = $release.published_at }
            }
        }
        Start-Sleep -Seconds 20
    }
    throw "Target public Release was not ready within $WaitForReleaseMinutes minutes"
}

try {
    $report["baselineResource"] = Assert-LiveVersion -Expected "1.0.0"
    $warmup = Start-LiveGui -Name "baseline-warmup"
    Assert-GuiResponsive -AppProcess $warmup -Seconds 3
    Close-Gui -AppProcess $warmup
    Invoke-LiveFixture -Arguments @("snapshot", "--root", $root)
    $checks.Add("Published baseline normalizes window preferences before all 11 profile files are snapshotted")

    $oldGui = Start-LiveGui -Name "baseline-real-update"
    $report["oldGuiPid"] = $oldGui.Id
    $null = Open-LiveAppSettings -AppProcess $oldGui -State "01-baseline"
    $initial = Wait-LiveButton -AppProcess $oldGui -Names @("Check for updates", "Download update") -State "02-startup-check-settled" -Seconds 90
    $report["startupSettledButton"] = $initial.Button.name
    Write-Output "Old v1.0.0 GUI is running; waiting for public v$TargetVersion."
    $published = Wait-PublicRelease -AppProcess $oldGui
    $report["publicRelease"] = $published

    # Inspect again at action time: if startup already found this release, do
    # not falsely label the existing available state as a manual update check.
    $discoveryDeadline = [DateTime]::UtcNow.AddMinutes(5)
    $nextCheckAt = [DateTime]::MinValue
    $available = $null
    while ([DateTime]::UtcNow -lt $discoveryDeadline) {
        $current = Wait-LiveButton -AppProcess $oldGui -Names @("Check for updates", "Download update") -State "03-public-release-discovery" -Seconds 60
        if ($current.Button.name -ceq "Download update") {
            $available = $current
            break
        }
        # GitHub's unauthenticated latest response may lag behind the observer's
        # authenticated response briefly. Retry the actual visible Check button
        # slowly and within a finite bound; never replace the app's API request.
        if ([DateTime]::UtcNow -lt $nextCheckAt) {
            $delay = [Math]::Max(1, [Math]::Ceiling(($nextCheckAt - [DateTime]::UtcNow).TotalMilliseconds))
            Start-Sleep -Milliseconds ([int]$delay)
            continue
        }
        $report["manualCheckCount"]++
        Invoke-LiveButton -AppProcess $oldGui -Button $current.Button -State "04-check-for-updates-$($report.manualCheckCount)"
        $report["manualCheckInvoked"] = $true
        $nextCheckAt = [DateTime]::UtcNow.AddSeconds(15)
    }
    if ($null -eq $available) { throw "The app did not discover the public target within five minutes of real UI checks" }
    if ($report.manualCheckInvoked) {
        $checks.Add("User-facing Check for updates discovers the newly published stable Release")
    }
    else {
        $report["backgroundCheckFoundTarget"] = $true
        $checks.Add("Production startup background check discovers the published stable Release")
    }
    $versionText = @($available.Tree.nodes | Where-Object { $_.PSObject.Properties.Name -contains "name" -and $_.name.Contains($TargetVersion) })
    if ($versionText.Count -eq 0) { throw "Available-update UI does not identify the intended target version" }
    Invoke-LiveButton -AppProcess $oldGui -Button $available.Button -State "06-download-update"
    $ready = Wait-LiveButton -AppProcess $oldGui -Names @("Restart and update") -State "07-downloaded-ready" -Seconds 900
    $checks.Add("Actual Download update action downloads and verifies the public Windows ZIP")

    try { Invoke-LiveButton -AppProcess $oldGui -Button $ready.Button -State "08-restart-and-update" }
    catch { $report["restartInvocationException"] = $_.Exception.Message }
    # Never close/kill the old GUI here: successful termination must be caused
    # by its own real Restart and update action and helper readiness handshake.
    if (-not $oldGui.WaitForExit(120000)) { throw "Old GUI did not exit itself after Restart and update" }
    if ($oldGui.ExitCode -ne 0) { throw "Old GUI exited with error during real update: $($oldGui.ExitCode)" }
    $report["oldGuiExitCode"] = $oldGui.ExitCode

    $newGui = $null
    $stage = $null
    $deadline = [DateTime]::UtcNow.AddSeconds(120)
    while ([DateTime]::UtcNow -lt $deadline) {
        $stages = @(Get-ChildItem -LiteralPath $install -Directory -Force | Where-Object { $_.Name.StartsWith(".gs-widget-update-") })
        foreach ($candidateStage in $stages) {
            $errorPath = Join-Path $candidateStage.FullName "error.txt"
            if (Test-Path -LiteralPath $errorPath) { throw "Real update helper failed: $(Get-Content -LiteralPath $errorPath -Raw)" }
            $completed = Join-Path $candidateStage.FullName "completed"
            if ((Test-Path -LiteralPath $completed) -and (Get-Content -LiteralPath $completed -Raw) -eq $TargetVersion) { $stage = $candidateStage.FullName }
        }
        $running = @(Get-Process -Name "gersang-widget" -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $oldGui.Id -and $_.Path -eq $executable })
        if ($running.Count -gt 1) { throw "Update unexpectedly started multiple installed GUI processes" }
        if ($null -ne $stage -and $running.Count -eq 1) { $newGui = $running[0]; break }
        Start-Sleep -Milliseconds 250
    }
    if ($null -eq $newGui) { throw "Actual updater did not produce a completed installation and restarted GUI" }
    $null = $newGui.Handle
    $tracked.Add($newGui)
    $report["newGuiPid"] = $newGui.Id
    $report["completedStage"] = $stage
    $report["installedResource"] = Assert-LiveVersion -Expected $TargetVersion
    Wait-Gui -AppProcess $newGui
    $newTree = Open-LiveAppSettings -AppProcess $newGui -State "09-restarted-version"
    $displayedVersion = @($newTree.nodes | Where-Object { $_.PSObject.Properties.Name -contains "name" -and $_.name -ceq "Gersang v$TargetVersion" })
    if ($displayedVersion.Count -eq 0) { throw "Restarted GUI does not display Gersang v$TargetVersion in App settings" }
    Close-Gui -AppProcess $newGui
    $checks.Add("Restart and update closes the old GUI normally, completes its own staged transaction and restarts a real new-version GUI")
    $checks.Add("Restarted App settings and Windows executable resources both report the target version")

    # This independent public download verifies what the app installed, after
    # the real app has already completed its own network download and update.
    $verificationDirectory = Join-Path $root "public release verification"
    New-Item -ItemType Directory -Path $verificationDirectory | Out-Null
    $candidateZip = Join-Path $verificationDirectory "gs-widget-v$TargetVersion-windows-x64.zip"
    Invoke-WebRequest -Uri $published.Url -OutFile $candidateZip -TimeoutSec 600
    if ((Get-Item -LiteralPath $candidateZip).Length -ne $published.Size) { throw "Independent public ZIP size differs from release metadata" }
    if ((Get-FileHash -LiteralPath $candidateZip -Algorithm SHA256).Hash.ToLowerInvariant() -ne $published.Sha256) { throw "Independent public ZIP SHA-256 differs from release metadata" }

    foreach ($name in @("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY")) { [Environment]::SetEnvironmentVariable($name, "http://127.0.0.1:1", "Process") }
    [Environment]::SetEnvironmentVariable("NO_PROXY", "", "Process")
    $offline = Start-LiveGui -Name "updated-offline"
    $null = Read-LiveUi -AppProcess $offline -State "10-updated-offline-startup"
    Assert-GuiResponsive -AppProcess $offline -Seconds 35
    $null = Read-LiveUi -AppProcess $offline -State "11-updated-offline-responsive"
    Close-Gui -AppProcess $offline
    Invoke-LiveFixture -Arguments @("verify", "--root", $root, "--candidate-zip", $candidateZip, "--version", $TargetVersion)
    $verification = Get-Content -LiteralPath (Join-Path $root "live-update-verification.json") -Raw -Encoding utf8 | ConvertFrom-Json
    if ($verification.app_generated_plan_parent_pid -ne $oldGui.Id) { throw "App-generated plan does not identify the real baseline GUI process" }
    $checks.Add("Updated GUI remains responsive offline for 35 seconds and closes normally")
    $checks.Add("All 11 personal and 13 unrelated files survive byte-for-byte; installed public ZIP, app-generated plan and original backup hashes match")
    $report["status"] = "passed"
    Write-Output "Real public GUI update passed: v1.0.0 -> v$TargetVersion, in-app download/restart, version UI, offline use and personal data verified."
}
catch {
    $report["status"] = "failed"
    $report["error"] = $_.Exception.Message
    throw
}
finally {
    # Cleanup is confined to test-owned executables after result/failure. The
    # upgrade's own old-GUI exit is asserted before any cleanup can reach here.
    foreach ($child in @(Get-Process -Name "gersang-widget", "updater" -ErrorAction SilentlyContinue)) {
        try {
            if ($child.Path -and $child.Path.StartsWith($root + [System.IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
                $child.Kill()
                $null = $child.WaitForExit(5000)
            }
        }
        catch { Write-Warning "Could not clean up a test process: $($_.Exception.Message)" }
        finally { $child.Dispose() }
    }
    foreach ($child in $tracked) { $child.Dispose() }
    foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name], "Process") }
    $observerToken = $null
    $report["finishedAtUtc"] = [DateTime]::UtcNow.ToString("o")
    Write-LiveJson -Path (Join-Path $root "live-update-report.json") -Value $report
    foreach ($log in @(Get-ChildItem -LiteralPath $logs -Filter "*.log")) {
        if ($log.Length -gt 0) { Write-Output "$($log.Name):"; Get-Content -LiteralPath $log.FullName }
    }
}
