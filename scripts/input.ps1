# Sends real mouse clicks / key presses to the Soundesti window (UI smoke testing).
# Coordinates are physical pixels relative to the window's top-left corner.
# Usage: scripts\input.ps1 "click:540,355" "key:SPACE" "wait:500" "rclick:600,700"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class In {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
}
"@
[In]::SetProcessDPIAware() | Out-Null
$p = Get-Process $(if ($env:SD_PROC) { $env:SD_PROC } else { "soundesti" }) | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $p) { throw "Soundesti window not found" }
[In]::SetForegroundWindow($p.MainWindowHandle) | Out-Null
Start-Sleep -Milliseconds 300
$r = New-Object In+RECT
[In]::GetWindowRect($p.MainWindowHandle, [ref]$r) | Out-Null
$vk = @{ SPACE = 0x20; ESC = 0x1B; F6 = 0x75; F7 = 0x76; F9 = 0x78; CTRL = 0x11; ALT = 0x12; SHIFT = 0x10 }
foreach ($a in $args) {
    $kind, $val = $a -split ":", 2
    switch ($kind) {
        { $_ -in "click", "rclick" } {
            $x, $y = $val -split ","
            [In]::SetCursorPos($r.L + [int]$x, $r.T + [int]$y) | Out-Null
            Start-Sleep -Milliseconds 60
            if ($kind -eq "click") { $d = 0x2; $u = 0x4 } else { $d = 0x8; $u = 0x10 }
            [In]::mouse_event($d, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60
            [In]::mouse_event($u, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 120
        }
        "key" {
            # key:CTRL+M style chords; single letters use their ASCII code.
            $codes = $val -split "\+" | ForEach-Object { if ($vk.ContainsKey($_)) { $vk[$_] } else { [byte][char]$_.ToUpper() } }
            foreach ($c in $codes) { [In]::keybd_event([byte]$c, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30 }
            [array]::Reverse($codes)
            foreach ($c in $codes) { [In]::keybd_event([byte]$c, 0, 2, [UIntPtr]::Zero); Start-Sleep -Milliseconds 30 }
            Start-Sleep -Milliseconds 150
        }
        "wait" { Start-Sleep -Milliseconds ([int]$val) }
    }
}
"done"

