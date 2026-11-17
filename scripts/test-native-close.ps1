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
$probe = Start-Process -FilePath (Resolve-Path -LiteralPath $Executable).Path -PassThru -WindowStyle Hidden
try {
    $deadline = (Get-Date).AddSeconds(15)
    do {
        Start-Sleep -Milliseconds 200
        $probe.Refresh()
        if ($probe.HasExited) { throw 'Application exited before opening its window.' }
    } while ($probe.MainWindowHandle -eq 0 -and (Get-Date) -lt $deadline)
    if ($probe.MainWindowHandle -eq 0) { throw 'Application did not create a native window.' }

    # Allow startup to finish, then issue the same WM_CLOSE as the title-bar X.
    # Retry while loading: the app deliberately protects in-flight operations,
    # and a cold WebView2 on a CI runner can take a while to load the page.
    Start-Sleep -Seconds 2
    $closed = $false
    for ($attempt = 1; $attempt -le 10; $attempt++) {
        [void][BeamForgeCloseProbe]::PostMessage($probe.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)
        if ($probe.WaitForExit(3000)) { $closed = $true; break }
    }
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
        throw "FAIL: Native close request left a clean window running ($state)."
    }
    if ($attempt -gt 1) { Write-Output "Closed on attempt $attempt." }
    if ($probe.ExitCode -ne 0) { throw "Window exited with error $($probe.ExitCode)." }
    Write-Output 'PASS: Native close request exits the application cleanly.'
} finally {
    if (-not $probe.HasExited) { Stop-Process -Id $probe.Id }
    $probe.Dispose()
}
