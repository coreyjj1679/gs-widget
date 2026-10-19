param(
    [Parameter(Mandatory = $true)][string]$ExePath,
    [Parameter(Mandatory = $true)][string]$RepositoryRoot,
    [Parameter(Mandatory = $true)][string]$TestRoot
)

# Exercise accessible settings/calculator controls and native navigation keys.
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

    private static Input KeyInput(ushort key, bool released)
    {
        var input = new Input();
        input.Type = 1; // INPUT_KEYBOARD
        input.Data.Keyboard.VirtualKey = key;
        input.Data.Keyboard.Flags = released ? 2u : 0u; // KEYEVENTF_KEYUP
        return input;
    }

    public static void Send(IntPtr window, ushort key, bool control, bool shift)
    {
        if (window == IntPtr.Zero)
            throw new InvalidOperationException("Cannot send a key without the app window");
        if (GetForegroundWindow() != window && !SetForegroundWindow(window))
            throw new InvalidOperationException("Cannot focus the app for a navigation key");
        if (GetForegroundWindow() != window)
            throw new InvalidOperationException("The app did not become the foreground window");
        var inputs = new Input[2 + (control ? 2 : 0) + (shift ? 2 : 0)];
        int index = 0;
        if (control) inputs[index++] = KeyInput(0x11, false); // VK_CONTROL
        if (shift) inputs[index++] = KeyInput(0x10, false); // VK_SHIFT
        inputs[index++] = KeyInput(key, false);
        inputs[index++] = KeyInput(key, true);
        if (shift) inputs[index++] = KeyInput(0x10, true);
        if (control) inputs[index++] = KeyInput(0x11, true);
        if (SendInput((uint)inputs.Length, inputs, Marshal.SizeOf(typeof(Input))) != inputs.Length)
        {
            int error = Marshal.GetLastWin32Error();
            // A partial send must not leave a modifier held for the next case.
            var release = new Input[] { KeyInput(key, true), KeyInput(0x10, true), KeyInput(0x11, true) };
            SendInput((uint)release.Length, release, Marshal.SizeOf(typeof(Input)));
            throw new InvalidOperationException("Cannot send navigation key: " + error);
        }
    }

    public static void PageDown(IntPtr window) { Send(window, 0x22, false, false); }
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

function Invoke-ObservedControl {
    param(
        [System.Diagnostics.Process]$AppProcess, $Tree, [string[]]$Names, [string]$Directory,
        [ValidateSet("Button", "TabItem")][string]$ControlType = "Button"
    )
    $controls = @($Tree.nodes | Where-Object {
        $_.PSObject.Properties.Name -contains "name" -and $_.controlType -eq "ControlType.$ControlType" -and
        $_.enabled -and -not $_.offscreen -and $Names -ccontains $_.name
    })
    if ($controls.Count -ne 1) { throw "Expected one observed $ControlType named $($Names -join ' / '), found $($controls.Count)" }
    $null = & (Join-Path $PSScriptRoot "windows_update_ui.ps1") -Mode Act -ProcessId $AppProcess.Id -OutputDirectory $Directory -ControlName $controls[0].name -ControlType $ControlType
}

function Ui-Names {
    param($Tree)
    return @($Tree.nodes | Where-Object { $_.PSObject.Properties.Name -contains "name" } | ForEach-Object { $_.name })
}

function Assert-UiNames {
    param($Tree, [string[]]$Required = @(), [string[]]$Absent = @())
    $names = @(Ui-Names -Tree $Tree)
    foreach ($name in $Required) {
        if ($names -cnotcontains $name) { throw "Expected UI text absent: $name" }
    }
    foreach ($name in $Absent) {
        if ($names -ccontains $name) { throw "Unexpected UI text present: $name" }
    }
}

function Assert-ModeControl {
    param(
        $Tree, [string]$Name, [bool]$Expected,
        [ValidateSet("Button", "TabItem")][string]$ControlType = "Button"
    )
    $modeControls = @($Tree.nodes | Where-Object {
        $_.PSObject.Properties.Name -contains "name" -and
        $_.controlType -eq "ControlType.$ControlType" -and $_.name -ceq $Name
    })
    if ($Expected) {
        if ($modeControls.Count -ne 1 -or -not $modeControls[0].enabled -or $modeControls[0].offscreen) {
            throw "Expected one usable mode-specific ${ControlType}: $Name"
        }
    } elseif ($modeControls.Count -ne 0) {
        throw "$ControlType should not exist in this window mode: $Name"
    }
}

function Read-AfterKey {
    param(
        [System.Diagnostics.Process]$AppProcess, [UInt16]$VirtualKey,
        [string]$Directory, [bool]$Control = $false, [bool]$Shift = $false
    )
    $window = [GersangSmokeWindow]::Find([uint32]$AppProcess.Id)
    [GersangChangelogKeys]::Send($window, $VirtualKey, $Control, $Shift)
    Start-Sleep -Milliseconds 400
    return Read-ChangelogUi -AppProcess $AppProcess -Directory $Directory
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
    scope = "Real Windows GUI: mode-specific calculator/Keys visibility, settings keyboard navigation, native PageDown/PageUp and readable bundled history in both languages and modes"
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
                $english = $language -eq "en-zh"
                $fullMode = $mode -eq "full"
                $calculateLabel = if ($english) { "Calculate" } else { "計算" }
                $calculatorHeading = if ($english) { "Crafting cost" } else { "合成成本" }
                $calculatorPickerPrefix = if ($english) { "Craftable item: " } else { "合成道具：" }
                $overviewLabel = if ($english) { "Overview" } else { "圖鑑總覽" }
                $overviewHeading = if ($english) { "Explore the compendium" } else { "探索巨商圖鑑" }
                $keysLabel = if ($english) { "Keys" } else { "快捷鍵" }
                $keysHeading = if ($english) { "Global keys" } else { "全域快捷鍵" }
                $aliasesLabel = if ($english) { "Aliases" } else { "別名" }
                $aliasesHeading = if ($english) { "Search aliases" } else { "搜尋別名" }
                $appLabel = if ($english) { "App" } else { "程式" }
                $initial = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "01-initial")
                Assert-ModeControl -Tree $initial -Name $calculateLabel -Expected $fullMode
                $caseResult["calculatorModeVisibility"] = $true
                if ($fullMode) {
                    Invoke-ObservedControl -AppProcess $appProcess -Tree $initial -Names @($calculateLabel) -Directory (Join-Path $directory "01a-open-calculator")
                    $initial = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "01b-calculator")
                    Assert-UiNames -Tree $initial -Required @($calculatorHeading) -Absent @($overviewHeading)
                    $pickers = @(Ui-Names -Tree $initial | Where-Object { $_.StartsWith($calculatorPickerPrefix, [StringComparison]::Ordinal) })
                    if ($pickers.Count -ne 1) { throw "Calculator did not expose one craftable-item picker" }
                }
                # Opening settings from the calculator must retain working keyboard
                # navigation and return to that calculator when settings is closed.
                Invoke-ObservedControl -AppProcess $appProcess -Tree $initial -Names @("設定 / Settings", "Settings", "設定") -Directory (Join-Path $directory "02-open-settings")
                $settings = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "03-settings")
                Assert-ModeControl -Tree $settings -Name $keysLabel -Expected (-not $fullMode) -ControlType TabItem
                $caseResult["keysModeVisibility"] = $true
                Invoke-ObservedControl -AppProcess $appProcess -Tree $settings -Names @($aliasesLabel) -ControlType TabItem -Directory (Join-Path $directory "03a-open-aliases")
                $aliases = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "03b-aliases")
                Assert-UiNames -Tree $aliases -Required @($aliasesHeading)
                $nextHeading = if ($fullMode) { "Gersang v$currentVersion" } else { $keysHeading }
                $forward = Read-AfterKey -AppProcess $appProcess -VirtualKey 0x09 -Control $true -Directory (Join-Path $directory "03c-ctrl-tab")
                Assert-UiNames -Tree $forward -Required @($nextHeading) -Absent @($aliasesHeading)
                $backward = Read-AfterKey -AppProcess $appProcess -VirtualKey 0x09 -Control $true -Shift $true -Directory (Join-Path $directory "03d-ctrl-shift-tab")
                Assert-UiNames -Tree $backward -Required @($aliasesHeading) -Absent @($nextHeading)
                # The active tab also supports the standard left/right arrow keys.
                $forward = Read-AfterKey -AppProcess $appProcess -VirtualKey 0x27 -Directory (Join-Path $directory "03e-right-arrow")
                Assert-UiNames -Tree $forward -Required @($nextHeading) -Absent @($aliasesHeading)
                $settings = Read-AfterKey -AppProcess $appProcess -VirtualKey 0x25 -Directory (Join-Path $directory "03f-left-arrow")
                Assert-UiNames -Tree $settings -Required @($aliasesHeading) -Absent @($nextHeading)
                $caseResult["settingsKeyboardNavigation"] = $true
                if ($fullMode) {
                    $calculator = Read-AfterKey -AppProcess $appProcess -VirtualKey 0x1B -Directory (Join-Path $directory "03g-close-settings")
                    Assert-UiNames -Tree $calculator -Required @($calculatorHeading) -Absent @($aliasesHeading)
                    # Calculator is a top-level destination. Escape dismisses
                    # local UI; the explicit Overview control returns home.
                    $calculator = Read-AfterKey -AppProcess $appProcess -VirtualKey 0x1B -Directory (Join-Path $directory "03h-calculator-escape")
                    Assert-UiNames -Tree $calculator -Required @($calculatorHeading) -Absent @($overviewHeading)
                    $caseResult["calculatorStaysOpenOnEscape"] = $true
                    Invoke-ObservedControl -AppProcess $appProcess -Tree $calculator -Names @($overviewLabel) -Directory (Join-Path $directory "03i-open-overview")
                    $overview = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "03j-overview")
                    Assert-UiNames -Tree $overview -Required @($overviewHeading) -Absent @($calculatorHeading)
                    $caseResult["calculatorOverviewNavigation"] = $true
                    Invoke-ObservedControl -AppProcess $appProcess -Tree $overview -Names @("設定 / Settings", "Settings", "設定") -Directory (Join-Path $directory "03k-reopen-settings")
                    $settings = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "03l-settings")
                }
                Invoke-ObservedControl -AppProcess $appProcess -Tree $settings -Names @($appLabel) -ControlType TabItem -Directory (Join-Path $directory "04-open-app")
                $appTree = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory "05-app-top")
                $expectedWidth = if ($mode -eq "full") { 960 } else { 560 }
                $expectedHeight = if ($mode -eq "full") { 640 } else { 600 }
                if ($appTree.screenshot.width -ne $expectedWidth -or $appTree.screenshot.height -ne $expectedHeight) {
                    throw "Unexpected window size: $($appTree.screenshot.width)x$($appTree.screenshot.height), expected ${expectedWidth}x${expectedHeight}"
                }
                $caseResult["windowSize"] = "$($appTree.screenshot.width)x$($appTree.screenshot.height)"
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
                # Cover real page-key navigation independently from the reading
                # scan: PageDown overlaps only 32px, less than a wrapped entry.
                $initialReadable = @(Visible-ChangelogNames -Tree $appTree | Where-Object { $expected.Contains($_) })
                $pageDown = Read-AfterKey -AppProcess $appProcess -VirtualKey 0x22 -Directory (Join-Path $directory "05a-history-page-down")
                $pagedReadable = @(Visible-ChangelogNames -Tree $pageDown | Where-Object { $expected.Contains($_) })
                $newlyReadable = @($pagedReadable | Where-Object { $initialReadable -cnotcontains $_ })
                $leftViewport = @($initialReadable | Where-Object { $pagedReadable -cnotcontains $_ })
                if ($newlyReadable.Count -eq 0 -or $leftViewport.Count -eq 0) { throw "PageDown did not move the changelog content" }
                $current = Read-AfterKey -AppProcess $appProcess -VirtualKey 0x21 -Directory (Join-Path $directory "05b-history-page-up")
                $restoredReadable = @(Visible-ChangelogNames -Tree $current)
                $notRestored = @($initialReadable | Where-Object { $restoredReadable -cnotcontains $_ })
                if ($notRestored.Count -gt 0) { throw "PageUp did not restore the initial readable changelog text: $($notRestored -join '; ')" }
                $caseResult["changelogPageKeys"] = $true
                $seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
                $visible = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
                $snapshots = [System.Collections.Generic.List[object]]::new()
                for ($sample = 0; $sample -lt 64; $sample++) {
                    $names = @(Ui-Names -Tree $current)
                    foreach ($name in $names) { $null = $seen.Add($name) }
                    foreach ($name in (Visible-ChangelogNames -Tree $current)) { $null = $visible.Add($name) }
                    foreach ($wrong in $otherLanguageText) {
                        if ($names -ccontains $wrong) { throw "Changelog contains the other language: $wrong" }
                    }
                    $snapshots.Add(@{ sample = $sample; screenshot = $current.screenshot.path; visibleNames = @(Visible-ChangelogNames -Tree $current) })
                    $missing = @($expected | Where-Object { -not $seen.Contains($_) })
                    $notVisible = @($expected | Where-Object { -not $visible.Contains($_) })
                    if ($missing.Count -eq 0 -and $notVisible.Count -eq 0) { break }
                    $window = [GersangSmokeWindow]::Find([uint32]$appProcess.Id)
                    # Four supported DownArrow inputs move 192px, retaining
                    # enough overlap to show each wrapped paragraph in full.
                    for ($step = 0; $step -lt 4; $step++) {
                        [GersangChangelogKeys]::Send($window, 0x28, $false, $false)
                    }
                    Start-Sleep -Milliseconds 500
                    $current = Read-ChangelogUi -AppProcess $appProcess -Directory (Join-Path $directory ("06-history-sample-{0:D2}" -f ($sample + 1)))
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
Write-Output "Offline native GUI checks passed for both languages and modes: calculator/Keys visibility, settings navigation, and readable changelogs."
