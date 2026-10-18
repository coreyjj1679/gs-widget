param(
    [Parameter(Mandatory = $true)][string]$ExePath,
    [Parameter(Mandatory = $true)][string]$RepositoryRoot,
    [Parameter(Mandatory = $true)][string]$TestRoot
)

# Exercise the real accessible Settings/App controls and native PageDown key.
# Each case uses a fresh disposable profile and a refused network proxy; the
# application has no special test entry point or replacement release endpoint.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw "Changelog smoke requires PowerShell 7 on Windows" }
. (Join-Path $PSScriptRoot "windows_gui_test_helpers.ps1")

if (-not ("GersangChangelogKeys" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class GersangChangelogKeys
{
    [StructLayout(LayoutKind.Sequential)]
    private struct Input { public uint Type; public InputUnion Data; }
    [StructLayout(LayoutKind.Explicit)]
    private struct InputUnion
    {
        [FieldOffset(0)] public MouseInput Mouse;
        [FieldOffset(0)] public KeyboardInput Keyboard;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct MouseInput
    {
        public int X, Y;
        public uint MouseData, Flags, Time;
        public UIntPtr ExtraInfo;
    }
    [StructLayout(LayoutKind.Sequential)]
    private struct KeyboardInput
    {
        public ushort VirtualKey, ScanCode;
        public uint Flags, Time;
        public UIntPtr ExtraInfo;
    }

    [DllImport("user32.dll", ExactSpelling = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll", ExactSpelling = true)]
    private static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    private static extern uint SendInput(uint count, Input[] inputs, int size);

    public static void PageDown(IntPtr window)
    {
        if (GetForegroundWindow() != window && !SetForegroundWindow(window))
            throw new InvalidOperationException("Cannot focus the app for the PageDown key");
        if (GetForegroundWindow() != window)
            throw new InvalidOperationException("The app did not become the foreground window");
        var inputs = new Input[2];
        inputs[0].Type = 1; // INPUT_KEYBOARD
        inputs[0].Data.Keyboard.VirtualKey = 0x22; // VK_NEXT / PageDown
        inputs[1].Type = 1;
        inputs[1].Data.Keyboard.VirtualKey = 0x22;
        inputs[1].Data.Keyboard.Flags = 2; // KEYEVENTF_KEYUP
        if (SendInput(2, inputs, Marshal.SizeOf(typeof(Input))) != 2)
            throw new InvalidOperationException("Cannot send PageDown: " + Marshal.GetLastWin32Error());
    }
}
'@
}

$utf8 = [System.Text.UTF8Encoding]::new($false)
function Write-Evidence {
    param([string]$Path, $Value)
    [System.IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 20), $utf8)
}

function Read-ChangelogUi {
    param([System.Diagnostics.Process]$AppProcess, [string]$Directory)
    $output = & (Join-Path $PSScriptRoot "windows_update_ui.ps1") -ProcessId $AppProcess.Id -OutputDirectory $Directory
    $summary = ($output -join "`n") | ConvertFrom-Json
    if (-not $summary.json -or -not $summary.screenshot.saved) { throw "UI inspection did not save both a tree and screenshot" }
    return Get-Content -Raw -LiteralPath $summary.json | ConvertFrom-Json
}

function Invoke-ObservedButton {
    param([System.Diagnostics.Process]$AppProcess, $Tree, [string[]]$Names, [string]$Directory)
    $buttons = @($Tree.nodes | Where-Object {
        $_.PSObject.Properties.Name -contains "name" -and $_.controlType -eq "ControlType.Button" -and
        $_.enabled -and -not $_.offscreen -and $Names -ccontains $_.name
    })
    if ($buttons.Count -ne 1) { throw "Expected one observed button named $($Names -join ' / '), found $($buttons.Count)" }
    $null = & (Join-Path $PSScriptRoot "windows_update_ui.ps1") -Mode Act -ProcessId $AppProcess.Id -OutputDirectory $Directory -ButtonName $buttons[0].name
}

function Ui-Names {
    param($Tree)
    return @($Tree.nodes | Where-Object { $_.PSObject.Properties.Name -contains "name" } | ForEach-Object { $_.name })
}

function Visible-ChangelogNames {
    param($Tree)
    # UI Automation offscreen information can include text beyond a Flickable's
    # clip. Intersect with the app's content viewport as well, and require the
    # complete vertical extent of the text to be on screen for readable evidence.
    $screen = $Tree.screenshot
    return @($Tree.nodes | Where-Object {
        $_.PSObject.Properties.Name -contains "name" -and -not $_.offscreen -and
        $_.bounds.height -gt 0 -and $_.bounds.width -gt 0 -and
        $_.bounds.y -ge ($screen.y + 138) -and
        ($_.bounds.y + $_.bounds.height) -le ($screen.y + $screen.height - 26) -and
        $_.bounds.x -ge $screen.x -and $_.bounds.x -lt ($screen.x + $screen.width)
    } | ForEach-Object { $_.name })
}

$executable = (Resolve-Path -LiteralPath $ExePath).Path
$repository = (Resolve-Path -LiteralPath $RepositoryRoot).Path
$root = [System.IO.Path]::GetFullPath($TestRoot)
if (Test-Path -LiteralPath $root) { throw "TestRoot must be a new disposable directory: $root" }
New-Item -ItemType Directory -Path $root | Out-Null
$notes = Get-Content -Raw -LiteralPath (Join-Path $repository "release-notes.json") | ConvertFrom-Json
$versionOutput = & python (Join-Path $repository "scripts/package_windows.py") --root $repository --version-only
if ($LASTEXITCODE -ne 0) { throw "Cannot determine current app version from the package manifest" }
$currentVersion = ($versionOutput -join "`n").Trim()
if (-not $currentVersion) { throw "Package manifest returned an empty app version" }
$cases = [System.Collections.Generic.List[object]]::new()
$report = [ordered]@{
    status = "running"
    appVersion = $currentVersion
    startedAtUtc = [DateTime]::UtcNow.ToString("o")
    network = "HTTP/HTTPS/ALL_PROXY point to refused localhost port 1; no endpoint override"
    scope = "Real Windows GUI, Settings/App buttons, native PageDown, bundled history in both languages and modes"
    cases = $cases
}
$overrides = @{
    LOCALAPPDATA = $null
    GERSANG_DB = Join-Path $repository "data/gersang.db"
    GERSANG_CLIENT = Join-Path $repository "data/client"
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
}

try {
    foreach ($name in $overrides.Keys) {
        [Environment]::SetEnvironmentVariable($name, $overrides[$name], "Process")
    }
    foreach ($language in @("en-zh", "zh-en")) {
        foreach ($mode in @("widget", "full")) {
            $caseName = "$language-$mode"
            $directory = Join-Path $root $caseName
            $caseProfile = Join-Path $directory "profile/gs-widget"
            New-Item -ItemType Directory -Path $caseProfile -Force | Out-Null
            $env:LOCALAPPDATA = Join-Path $directory "profile"
            [System.IO.File]::WriteAllText((Join-Path $caseProfile "ui-lang.txt"), $language, $utf8)
            [System.IO.File]::WriteAllText((Join-Path $caseProfile "opacity.txt"), "100", $utf8)
            [System.IO.File]::WriteAllText((Join-Path $caseProfile "window.txt"), "size=560,600`npos=32,32`nmode=$mode`nfull-pos=32,32`nfull-size=960,640`n", $utf8)
            $caseResult = [ordered]@{ name = $caseName; passed = $false; language = $language; mode = $mode }
            $cases.Add($caseResult)
            $appProcess = $null
            try {
                $appProcess = Start-Process -FilePath $executable -WorkingDirectory $directory -PassThru -RedirectStandardOutput (Join-Path $directory "stdout.log") -RedirectStandardError (Join-Path $directory "stderr.log")
                Wait-Gui -AppProcess $appProcess
                Assert-GuiResponsive -AppProcess $appProcess -Seconds 3
                $caseResult["processId"] = $appProcess.Id
                $initial = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "01-initial")
                Invoke-ObservedButton -AppProcess $appProcess -Tree $initial -Names @("設定 / Settings", "Settings", "設定") -Directory (Join-Path $directory "02-open-settings")
                $settings = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "03-settings")
                $appLabel = if ($language -eq "en-zh") { "App" } else { "程式" }
                Invoke-ObservedButton -AppProcess $appProcess -Tree $settings -Names @($appLabel) -Directory (Join-Path $directory "04-open-app")
                $appTree = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "05-app-top")
                $expectedWidth = if ($mode -eq "full") { 960 } else { 560 }
                $expectedHeight = if ($mode -eq "full") { 640 } else { 600 }
                if ($appTree.screenshot.width -ne $expectedWidth -or $appTree.screenshot.height -ne $expectedHeight) {
                    throw "Unexpected window size: $($appTree.screenshot.width)x$($appTree.screenshot.height), expected ${expectedWidth}x${expectedHeight}"
                }
                $caseResult["windowSize"] = "$($appTree.screenshot.width)x$($appTree.screenshot.height)"
                $english = $language -eq "en-zh"
                $languageKey = if ($english) { "en" } else { "zh-TW" }
                $otherLanguageKey = if ($english) { "zh-TW" } else { "en" }
                $heading = if ($english) { "Changelog" } else { "更新日誌" }
                $installed = if ($english) { "Installed" } else { "目前版本" }
                $expected = [System.Collections.Generic.List[string]]::new()
                $expected.Add($heading)
                $expected.Add($installed)
                $otherLanguageText = [System.Collections.Generic.List[string]]::new()
                foreach ($release in $notes.releases) {
                    $expected.Add("v$($release.version)")
                    foreach ($section in $release.sections) {
                        foreach ($entry in $section.$languageKey) { $expected.Add($entry) }
                        foreach ($entry in $section.$otherLanguageKey) { $otherLanguageText.Add($entry) }
                    }
                }
                $names = @(Ui-Names -Tree $appTree)
                if ($names -cnotcontains "Gersang v$currentVersion") { throw "App settings does not show the installed version" }
                $seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
                $visible = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
                $snapshots = [System.Collections.Generic.List[object]]::new()
                $current = $appTree
                for ($page = 0; $page -lt 8; $page++) {
                    $names = @(Ui-Names -Tree $current)
                    foreach ($name in $names) { $null = $seen.Add($name) }
                    foreach ($name in (Visible-ChangelogNames -Tree $current)) { $null = $visible.Add($name) }
                    foreach ($wrong in $otherLanguageText) {
                        if ($names -ccontains $wrong) { throw "Changelog contains the other language: $wrong" }
                    }
                    $snapshots.Add(@{ page = $page; screenshot = $current.screenshot.path; visibleNames = @(Visible-ChangelogNames -Tree $current) })
                    $missing = @($expected | Where-Object { -not $seen.Contains($_) })
                    $notVisible = @($expected | Where-Object { -not $visible.Contains($_) })
                    if ($missing.Count -eq 0 -and $notVisible.Count -eq 0) { break }
                    $window = [GersangSmokeWindow]::Find([uint32]$appProcess.Id)
                    [GersangChangelogKeys]::PageDown($window)
                    Start-Sleep -Milliseconds 500
                    $current = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory ("06-history-page-{0:D2}" -f ($page + 1)))
                }
                $caseResult["snapshots"] = $snapshots
                $caseResult["missingAccessibleText"] = $missing
                $caseResult["missingReadableText"] = $notVisible
                if ($missing.Count -gt 0) { throw "Expected translated history text absent from UI Automation: $($missing -join '; ')" }
                if ($notVisible.Count -gt 0) { throw "History text never fully visible while scrolling: $($notVisible -join '; ')" }
                Assert-GuiResponsive -AppProcess $appProcess -Seconds 5
                Close-Gui -AppProcess $appProcess
                $caseResult["normalExit"] = $true
                if ([System.IO.File]::ReadAllText((Join-Path $caseProfile "ui-lang.txt")) -cne $language) {
                    throw "Reading changelog changed the saved language"
                }
                $caseResult["verifiedHistoryTextCount"] = $expected.Count
                $caseResult["passed"] = $true
                Write-Host "${caseName}: all $($expected.Count) expected changelog texts were accessible and readable offline."
            }
            catch {
                $caseResult["error"] = $_.Exception.ToString()
                Write-Warning "Changelog case $caseName failed: $($_.Exception.Message)"
            }
            finally {
                if ($null -ne $appProcess) {
                    try {
                        $appProcess.Refresh()
                        if (-not $appProcess.HasExited) {
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
                Write-Evidence -Path (Join-Path $directory "observations.json") -Value $caseResult
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
    Write-Evidence -Path (Join-Path $root "changelog-report.json") -Value $report
}
if ($report.status -ne "passed") { throw "Bilingual changelog smoke failed; inspect changelog-report.json and screenshots" }
Write-Output "English and Traditional Chinese changelogs were readable offline in widget and full modes."
