param(
    [string]$Executable = "$PSScriptRoot/../target/release/beam-forge.exe",
    [string]$FailureScreenshot = "$PSScriptRoot/../target/close-test-failure.png"
)

$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class BeamForgeCloseProbe {
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
    [DllImport("user32.dll")]
    public static extern bool IsWindow(IntPtr window);
    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr window, IntPtr hdc, uint flags);
    public struct Rect { public int Left, Top, Right, Bottom; }
}
'@

# Only the fresh process created here is closed. Existing user sessions are untouched.
$started = Get-Date
$probe = Start-Process -FilePath (Resolve-Path -LiteralPath $Executable).Path -PassThru -WindowStyle Hidden
try {
    $deadline = (Get-Date).AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 200
        $probe.Refresh()
        if ($probe.HasExited) { throw 'Application exited before opening its window.' }
    } while ($probe.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline)
    if ($probe.MainWindowHandle -eq 0) { throw 'Application did not create a native window.' }

    # Wait for the frontend to mount before closing: once it registers its
    # close handler, Tauri holds each close until the page answers it. The
    # page renames the window to "<project> – Beam Forge" when it mounts.
    $readySuffix = " $([char]0x2013) Beam Forge"
    $deadline = $started.AddSeconds(60)
    while (-not "$($probe.MainWindowTitle)".EndsWith($readySuffix) -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 200
        $probe.Refresh()
        if ($probe.HasExited) { throw "Application exited before its frontend mounted (exit code $($probe.ExitCode))." }
    }
    if (-not "$($probe.MainWindowTitle)".EndsWith($readySuffix)) {
        throw "FAIL: Frontend never mounted (title: '$($probe.MainWindowTitle)')."
    }
    $ready = Get-Date
    $toReady = ($ready - $started).TotalSeconds

    # Issue the same WM_CLOSE as the title-bar X. A loaded, clean app should
    # exit on the first one; a retry passes but is flagged so it isn't silent.
    $closed = $false
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        if ($attempt -gt 1) {
            $sinceReady = ((Get-Date) - $ready).TotalSeconds
            Write-Output ("::warning title=Native close retry::Close attempt {0} of 3: the previous WM_CLOSE was ignored ({1:N1} s after the frontend mounted)." -f $attempt, $sinceReady)
        }
        [void][BeamForgeCloseProbe]::PostMessage($probe.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
        if ($probe.WaitForExit(10000)) { $closed = $true; break }
    }
    $toExit = ((Get-Date) - $ready).TotalSeconds
    if (-not $closed) {
        # Say which half failed: the window ignoring the close, or the process
        # hanging on exit after its window went away. Then show the
        # window and capture it, to see whether the close guard's dialog is up.
        $window = [BeamForgeCloseProbe]::IsWindow($probe.MainWindowHandle)
        $probe.Refresh()
        $state = "window exists: $window, UI thread responding: $($probe.Responding), threads: $($probe.Threads.Count)"
        if ($window) {
            try {
                [void][BeamForgeCloseProbe]::ShowWindow($probe.MainWindowHandle, 1)
                Start-Sleep -Seconds 2
                $rect = New-Object BeamForgeCloseProbe+Rect
                [void][BeamForgeCloseProbe]::GetWindowRect($probe.MainWindowHandle, [ref]$rect)
                Add-Type -AssemblyName System.Drawing
                $bitmap = New-Object System.Drawing.Bitmap ($rect.Right - $rect.Left), ($rect.Bottom - $rect.Top)
                $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
                # PW_RENDERFULLCONTENT captures the WebView even when covered.
                $hdc = $graphics.GetHdc()
                [void][BeamForgeCloseProbe]::PrintWindow($probe.MainWindowHandle, $hdc, 2)
                $graphics.ReleaseHdc($hdc)
                $bitmap.Save($FailureScreenshot)
                $graphics.Dispose(); $bitmap.Dispose()
                $state += ", screenshot: $FailureScreenshot"
            } catch {
                $state += ", screenshot failed: $_"
            }
        }
        throw ("FAIL: Native close request left a clean window running ({0:N1} s after ready, {1})." -f $toExit, $state)
    }
    if ($probe.ExitCode -ne 0) { throw "Window exited with error $($probe.ExitCode)." }
    Write-Output ("Time to ready: {0:N1} s after launch. Time to close: {1:N1} s after ready, on attempt {2}." -f $toReady, $toExit, $attempt)
    Write-Output 'PASS: Native close request exits the application cleanly.'
} finally {
    if (-not $probe.HasExited) { Stop-Process -Id $probe.Id }
    $probe.Dispose()
}
