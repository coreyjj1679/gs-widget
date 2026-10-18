param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [ValidateSet("Inspect", "Act")][string]$Mode = "Inspect",
    [Alias("ButtonName")][string]$ControlName,
    [ValidateSet("Button", "TabItem")][string]$ControlType = "Button",
    [string]$AutomationId,
    [ValidateRange(1, 4000)][int]$MaximumNodes = 1200
)

# Test-only UI Automation client. Act accepts exact selectors observed through
# Inspect; it never substitutes a helper command or modifies application state
# outside the application's own accessible Invoke or Select action.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ($PSVersionTable.PSEdition -eq "Core") {
    if (-not $IsWindows) { throw "Windows UI Automation requires Windows" }
    # UIAutomationClient/UIAutomationTypes are .NET Framework assemblies on the
    # hosted runner. Use the installed Desktop PowerShell runtime for that client
    # rather than loading framework WPF assemblies into PowerShell 7's runtime.
    $start = [System.Diagnostics.ProcessStartInfo]::new()
    $start.FileName = Join-Path $env:WINDIR "System32/WindowsPowerShell/v1.0/powershell.exe"
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [System.Text.Encoding]::UTF8
    $start.StandardErrorEncoding = [System.Text.Encoding]::UTF8
    foreach ($argument in @("-NoProfile", "-NonInteractive", "-Mta", "-ExecutionPolicy", "Bypass", "-File", $PSCommandPath, "-Mode", $Mode, "-ProcessId", "$ProcessId", "-OutputDirectory", $OutputDirectory, "-MaximumNodes", "$MaximumNodes", "-ControlType", $ControlType)) {
        $start.ArgumentList.Add($argument)
    }
    if ($ControlName) { $start.ArgumentList.Add("-ControlName"); $start.ArgumentList.Add($ControlName) }
    if ($AutomationId) { $start.ArgumentList.Add("-AutomationId"); $start.ArgumentList.Add($AutomationId) }
    $worker = [System.Diagnostics.Process]::new()
    $worker.StartInfo = $start
    try {
        if (-not $worker.Start()) { throw "Cannot start Desktop UI Automation worker" }
        $stdout = $worker.StandardOutput.ReadToEndAsync()
        $stderr = $worker.StandardError.ReadToEndAsync()
        if (-not $worker.WaitForExit(60000)) {
            $worker.Kill()
            $null = $worker.WaitForExit(5000)
            throw "UI Automation worker exceeded its 60-second bound"
        }
        $output = $stdout.GetAwaiter().GetResult()
        $errors = $stderr.GetAwaiter().GetResult()
        if ($output) { Write-Output $output.TrimEnd() }
        if ($errors) { Write-Warning $errors.TrimEnd() }
        if ($worker.ExitCode -ne 0) { throw "UI Automation worker failed with exit code $($worker.ExitCode)" }
    }
    finally { $worker.Dispose() }
    return
}

# Windows PowerShell 5.1 has no automatic IsWindows variable. The existing
# native window helper intentionally requires it before exposing Win32 calls.
$IsWindows = $true
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
# The shared helper contains the Chinese native window title. Windows
# PowerShell treats BOM-less .ps1 files as ANSI, so decode its UTF-8 explicitly.
$helperSource = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot "windows_gui_test_helpers.ps1"), [System.Text.Encoding]::UTF8)
. ([scriptblock]::Create($helperSource))
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

if ($Mode -eq "Act" -and -not $ControlName -and -not $AutomationId) {
    throw "Act requires an exact ControlName (or ButtonName alias) or AutomationId from an observed UI tree"
}
$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$prefix = "{0}-{1}-{2}" -f [DateTime]::UtcNow.ToString("yyyyMMdd-HHmmss-fff"), $Mode.ToLowerInvariant(), $ProcessId
$utf8 = [System.Text.UTF8Encoding]::new($false)

function Write-UiJson {
    param([string]$Path, $Value)
    [System.IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 16), $utf8)
}

function Save-WindowScreenshot {
    param([System.Windows.Automation.AutomationElement]$Element, [string]$Path)
    $bounds = $Element.Current.BoundingRectangle
    if ($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0) {
        return @{ saved = $false; reason = "Window has no visible bounds" }
    }
    $windowBounds = [System.Drawing.Rectangle]::new(
        [int][Math]::Floor($bounds.X), [int][Math]::Floor($bounds.Y),
        [int][Math]::Ceiling($bounds.Width), [int][Math]::Ceiling($bounds.Height))
    $capture = [System.Drawing.Rectangle]::Intersect($windowBounds, [System.Windows.Forms.SystemInformation]::VirtualScreen)
    if ($capture.Width -le 0 -or $capture.Height -le 0) {
        return @{ saved = $false; reason = "Window is outside the current virtual screen" }
    }
    $bitmap = [System.Drawing.Bitmap]::new($capture.Width, $capture.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($capture.Location, [System.Drawing.Point]::Empty, $capture.Size)
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally { $graphics.Dispose(); $bitmap.Dispose() }
    return @{ saved = $true; path = $Path; x = $capture.X; y = $capture.Y; width = $capture.Width; height = $capture.Height }
}

function Read-UiTree {
    param([string]$Label)
    $process = [System.Diagnostics.Process]::GetProcessById($ProcessId)
    try {
        $window = [GersangSmokeWindow]::Find([uint32]$ProcessId)
        if ($window -eq [IntPtr]::Zero) { throw "No visible Gersang app window belongs to process $ProcessId" }
        $rootElement = [System.Windows.Automation.AutomationElement]::FromHandle($window)
        $cache = [System.Windows.Automation.CacheRequest]::new()
        $cache.TreeScope = [System.Windows.Automation.TreeScope]::Element
        foreach ($property in @(
            [System.Windows.Automation.AutomationElement]::NameProperty,
            [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
            [System.Windows.Automation.AutomationElement]::ClassNameProperty,
            [System.Windows.Automation.AutomationElement]::HelpTextProperty,
            [System.Windows.Automation.AutomationElement]::IsEnabledProperty,
            [System.Windows.Automation.AutomationElement]::IsOffscreenProperty,
            [System.Windows.Automation.AutomationElement]::IsControlElementProperty,
            [System.Windows.Automation.AutomationElement]::IsContentElementProperty,
            [System.Windows.Automation.AutomationElement]::BoundingRectangleProperty,
            [System.Windows.Automation.AutomationElement]::ProcessIdProperty
        )) { $cache.Add($property) }

        # AccessKit initializes its provider tree on the first UIA request and
        # posts that request to the app's event loop. Allow that first activation
        # to settle instead of concluding that a root-only tree has no controls.
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        do {
            $activation = $cache.Activate()
            try {
                $elements = $rootElement.FindAll([System.Windows.Automation.TreeScope]::Subtree, [System.Windows.Automation.Condition]::TrueCondition)
            }
            finally { $activation.Dispose() }
            if ($elements.Count -gt 1) { break }
            Start-Sleep -Milliseconds 200
        } while ([DateTime]::UtcNow -lt $deadline)

        $nodes = [System.Collections.Generic.List[object]]::new()
        $limit = [Math]::Min($elements.Count, $MaximumNodes)
        # Newer UIA descriptions are optional on the managed Framework client.
        # ID 30159 is UIA_FullDescriptionPropertyId in the pinned Windows SDK.
        $fullDescriptionProperty = [System.Windows.Automation.AutomationProperty]::LookupById(30159)
        $walkStarted = [DateTime]::UtcNow
        for ($index = 0; $index -lt $limit; $index++) {
            if (([DateTime]::UtcNow - $walkStarted).TotalSeconds -gt 20) { break }
            $element = $elements.Item($index)
            try {
                $info = $element.Cached
                $bounds = $info.BoundingRectangle
                $parentId = $null
                try {
                    $parent = [System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($element)
                    if ($null -ne $parent) { $parentId = $parent.GetRuntimeId() -join "," }
                }
                catch { $parentId = "unavailable" }
                $description = $null
                if ($null -ne $fullDescriptionProperty) {
                    $description = $element.GetCurrentPropertyValue($fullDescriptionProperty, $true)
                    if ($description -eq [System.Windows.Automation.AutomationElement]::NotSupported) { $description = $null }
                }
                $patterns = @($element.GetSupportedPatterns() | ForEach-Object { $_.ProgrammaticName })
                $nodes.Add([ordered]@{
                    index = $index
                    runtimeId = ($element.GetRuntimeId() -join ",")
                    parentRuntimeId = $parentId
                    name = $info.Name
                    automationId = $info.AutomationId
                    controlType = $info.ControlType.ProgrammaticName
                    className = $info.ClassName
                    helpText = $info.HelpText
                    fullDescription = $description
                    enabled = $info.IsEnabled
                    offscreen = $info.IsOffscreen
                    isControlElement = $info.IsControlElement
                    isContentElement = $info.IsContentElement
                    processId = $info.ProcessId
                    bounds = @{ x = $bounds.X; y = $bounds.Y; width = $bounds.Width; height = $bounds.Height }
                    supportedPatterns = $patterns
                })
            }
            catch {
                $nodes.Add([ordered]@{ index = $index; stale = $true; error = $_.Exception.Message })
            }
        }
        $snapshot = [ordered]@{
            capturedAtUtc = [DateTime]::UtcNow.ToString("o")
            processId = $ProcessId
            executable = $process.MainModule.FileName
            windowHandle = $window.ToInt64()
            nodeCount = $elements.Count
            capturedNodeCount = $nodes.Count
            truncated = $nodes.Count -lt $elements.Count
            frameworkSupportsFullDescription = $null -ne $fullDescriptionProperty
            nodes = $nodes
        }
        $screenshotPath = Join-Path $outputRoot "$prefix-$Label.png"
        try { $snapshot["screenshot"] = Save-WindowScreenshot -Element $rootElement -Path $screenshotPath }
        catch { $snapshot["screenshot"] = @{ saved = $false; reason = $_.Exception.Message } }
        $jsonPath = Join-Path $outputRoot "$prefix-$Label.json"
        Write-UiJson -Path $jsonPath -Value $snapshot
        return @{ Snapshot = $snapshot; Elements = $elements; JsonPath = $jsonPath }
    }
    finally { $process.Dispose() }
}

$before = Read-UiTree -Label "before"
if ($Mode -eq "Inspect") {
    [ordered]@{ mode = $Mode; processId = $ProcessId; json = $before.JsonPath; screenshot = $before.Snapshot.screenshot; nodes = $before.Snapshot.capturedNodeCount } | ConvertTo-Json -Depth 5
    return
}

$expectedControlType = "ControlType.$ControlType"
$targets = @($before.Snapshot.nodes | Where-Object {
    $_.Contains("name") -and $_.controlType -eq $expectedControlType -and $_.enabled -and -not $_.offscreen -and
    (-not $ControlName -or $_.name -ceq $ControlName) -and
    (-not $AutomationId -or $_.automationId -ceq $AutomationId)
})
if ($targets.Count -ne 1) {
    throw "Expected exactly one visible enabled $ControlType matching the observed selector; found $($targets.Count). Inspect $($before.JsonPath)"
}
$target = $targets[0]
$selectedElement = $before.Elements.Item([int]$target.index)
$actionPattern = $null
$actionMethod = "Invoke"
if ($ControlType -eq "TabItem" -and $selectedElement.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern, [ref]$actionPattern)) {
    $actionMethod = "Select"
} elseif (-not $selectedElement.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$actionPattern)) {
    throw "Selected $ControlType does not expose a supported Select/Invoke action; inspect its supported patterns before choosing another action"
}
$action = [ordered]@{
    mode = "Act"
    processId = $ProcessId
    selectedNode = $target
    actionMethod = $actionMethod
    beforeJson = $before.JsonPath
    invokedAtUtc = [DateTime]::UtcNow.ToString("o")
    outcome = "pending"
}
$actionPath = Join-Path $outputRoot "$prefix-action.json"
Write-UiJson -Path $actionPath -Value $action
try {
    if ($actionMethod -eq "Select") { $actionPattern.Select() }
    else { $actionPattern.Invoke() }
    $action["outcome"] = "$($actionMethod.ToLowerInvariant())-returned"
    Start-Sleep -Milliseconds 400
    $stillRunning = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $stillRunning) {
        $action["appExited"] = $true
    }
    else {
        $stillRunning.Dispose()
        try { $after = Read-UiTree -Label "after"; $action["afterJson"] = $after.JsonPath }
        catch { $action["afterInspectionError"] = $_.Exception.Message }
    }
}
catch {
    # Invoking Restart may destroy the provider while Invoke is returning. Keep
    # that ambiguous outcome explicit; the caller must verify process handoff
    # and the new application before claiming that the update succeeded.
    $action["outcome"] = "$($actionMethod.ToLowerInvariant())-error-check-process-state"
    $action["error"] = $_.Exception.Message
    throw
}
finally { Write-UiJson -Path $actionPath -Value $action }
[ordered]@{ mode = $Mode; processId = $ProcessId; actionJson = $actionPath; outcome = $action.outcome } | ConvertTo-Json -Depth 4
