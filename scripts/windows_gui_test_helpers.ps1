# Winit owns an additional visible, untitled event-loop window. Select the real
# application window by title and process ID, as widget/src/window/windows.rs
# does, instead of relying on Process.MainWindowHandle's first-window heuristic.
if (-not $IsWindows) { throw "GUI smoke helpers require PowerShell 7 on Windows" }

if (-not ("GersangSmokeWindow" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class GersangSmokeWindow
{
    [DllImport("user32.dll", CharSet = CharSet.Unicode, ExactSpelling = true, SetLastError = true)]
    private static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after,
        string windowClass, string windowTitle);

    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll", ExactSpelling = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    private static extern IntPtr SendMessageTimeoutW(IntPtr window, uint message,
        UIntPtr wParam, IntPtr lParam, uint flags, uint timeout, out UIntPtr result);

    [DllImport("user32.dll", ExactSpelling = true, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool PostMessageW(IntPtr window, uint message,
        UIntPtr wParam, IntPtr lParam);

    public static IntPtr Find(uint processId)
    {
        IntPtr window = IntPtr.Zero;
        while ((window = FindWindowExW(IntPtr.Zero, window, null, "巨商")) != IntPtr.Zero)
        {
            uint owner;
            if (GetWindowThreadProcessId(window, out owner) != 0 &&
                owner == processId && IsWindowVisible(window))
            {
                return window;
            }
        }
        return IntPtr.Zero;
    }

    public static bool Responds(IntPtr window)
    {
        UIntPtr result;
        // WM_NULL; SMTO_BLOCK | SMTO_ABORTIFHUNG. Every probe is bounded;
        // deliberately omit SMTO_NOTIMEOUTIFNOTHUNG, which removes that bound.
        return SendMessageTimeoutW(window, 0, UIntPtr.Zero, IntPtr.Zero,
            0x0001 | 0x0002, 1000, out result) != IntPtr.Zero;
    }

    public static bool RequestClose(IntPtr window)
    {
        return PostMessageW(window, 0x0010, UIntPtr.Zero, IntPtr.Zero); // WM_CLOSE
    }
}
'@
}

function Wait-Gui {
    param([System.Diagnostics.Process]$AppProcess, [int]$Seconds = 30)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $AppProcess.Refresh()
        if ($AppProcess.HasExited) { throw "GUI exited during startup: $($AppProcess.Id), exit $($AppProcess.ExitCode)" }
        $window = [GersangSmokeWindow]::Find([uint32]$AppProcess.Id)
        if ($window -ne [IntPtr]::Zero -and [GersangSmokeWindow]::Responds($window)) { return }
        Start-Sleep -Milliseconds 200
    }
    throw "Visible 巨商 window for process $($AppProcess.Id) did not become responsive within $Seconds seconds"
}

function Assert-GuiResponsive {
    param([System.Diagnostics.Process]$AppProcess, [int]$Seconds)
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $AppProcess.Refresh()
        if ($AppProcess.HasExited) { throw "GUI exited during observation: $($AppProcess.Id)" }
        $window = [GersangSmokeWindow]::Find([uint32]$AppProcess.Id)
        if ($window -eq [IntPtr]::Zero -or -not [GersangSmokeWindow]::Responds($window)) {
            throw "Visible 巨商 window stopped responding during observation: $($AppProcess.Id)"
        }
        Start-Sleep -Milliseconds 200
    }
}

function Close-Gui {
    param([System.Diagnostics.Process]$AppProcess)
    $AppProcess.Refresh()
    if ($AppProcess.HasExited) { throw "GUI exited before normal shutdown was requested: $($AppProcess.Id)" }
    $window = [GersangSmokeWindow]::Find([uint32]$AppProcess.Id)
    if ($window -eq [IntPtr]::Zero -or -not [GersangSmokeWindow]::RequestClose($window)) {
        throw "Could not request normal 巨商 window shutdown: $($AppProcess.Id)"
    }
    if (-not $AppProcess.WaitForExit(10000)) { throw "GUI did not close normally within 10 seconds" }
    if ($AppProcess.ExitCode -ne 0) { throw "GUI closed with error: $($AppProcess.ExitCode)" }
}
