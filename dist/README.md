# Luma

Luma is a small, dependency-free Windows monitor utility written in Rust. It combines
hardware monitor control (DDC/CI), Windows brightness and gamma control, a
sunrise/sunset-aware night-light scheduler, and a time-based auto-dim curve in one
~400 KB executable.

Version 0.4.0.

## Requirements

- Windows 10 or 11 (x64).
- DDC/CI must be enabled on the monitor itself for hardware brightness/contrast/input
  controls. Many monitors call this "DDC/CI" in their OSD; laptops and virtual/remote
  displays often do not expose it.
- The executable is self-contained. No .NET, no Visual C++ redistributable, no installer.

## Running

Double-click `Luma.exe`. Settings are stored in `%APPDATA%\Luma\settings.ini`.

On startup Luma enumerates the monitors and reads their DDC/CI capabilities. If a
control is unsupported, the corresponding read/write returns an error in the status
line instead of failing silently.

### Integrity

This build ships with a **Medium** integrity label on both `Luma.exe` and the ZIP, so
double-clicking it from Explorer runs with normal desktop rights. If you copy the exe
out of a Low-integrity folder (for example, an agent sandbox or a locked-down download
directory), the copy can inherit that Low label. Extract the ZIP with Explorer or copy
the exe into a normal folder such as `%LOCALAPPDATA%\Luma` to keep Medium integrity.
From an ordinary PowerShell window you can also raise the label explicitly:

```powershell
icacls .\Luma.exe /setintegritylevel Medium
```

## Features

### Displays page

- Per-monitor brightness slider (DDC/CI VCP `0x10`) with Windows panel-brightness
  fallback where available.
- Brightness hotkeys: `Ctrl+Alt+Up` / `Ctrl+Alt+Down`.
- VCP control browser: 13 common controls (brightness, contrast, input source, volume,
  color preset, power mode, sharpness, red/green/blue gain, etc.) plus any custom
  `0x00`–`0xFF` VCP code. Read current value, set a value, or enumerate every
  supported code on that monitor.
- Gamma ramp read/write per display.

### Night light page

- Modes:
  - **Sun** — sunrise/sunset computed locally from latitude and longitude (NOAA
    algorithm), with configurable offsets before/after sunset and sunrise.
  - **Custom** — separate weekday and weekend on/off windows, including windows that
    cross midnight.
  - **Always** — keep the filter active permanently.
- Color temperature (1000–6500 K, Tanner Helland mapping) and strength (0–100%).
- Fade duration (0–240 min) so the ramp eases in and out instead of snapping.
- "Pause 1 hour" button; pauses survive restarts.
- Toggle the filter globally with `Ctrl+Alt+N`.
- Live status showing the current mode, next transition, and computed sunrise/sunset.

### Auto dim page

- Daily brightness curve: evening start and level, night start and level, and the
  morning anchor when daytime returns to 100%.
- Linear fades over the configured minutes, applied through DDC/CI hardware brightness
  rather than gamma, so colors are not shifted.
- Live "Right now" target, current phase, and countdown to the next change.
- Manual slider changes pause auto dim for five minutes so a drag is never fought.
- Displays without DDC/CI are skipped safely; options are persisted and restored.

### Settings page

- Light and dark themes, switched live from the "Dark theme" checkbox and remembered
  across restarts.
- Latitude, longitude and UTC offset for the solar calculation.
- Minimize-to-tray and run-at-login (per-user registry entry).
- Open settings folder / reset to defaults.

## Tray icon

Closing the window minimizes to the notification area when that option is enabled.
The tray menu provides Show, Toggle night light, Pause, and Exit.

## Exit codes and diagnostics

```powershell
Luma.exe --diagnose *> diag.txt
Get-Content diag.txt
```

prints every display, its device name, physical monitor count, DDC/CI handshake result,
and the VCP codes the display reports. Use it first when a monitor control does not
work. The release binary has no console window, so redirect the output as shown.

## Build from source

Prerequisites: Rust (MSVC toolchain) and Visual Studio 2022 Build Tools with the
Windows SDK.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\build.ps1 build
powershell -NoProfile -ExecutionPolicy Bypass -File .\build.ps1 build --release
powershell -NoProfile -ExecutionPolicy Bypass -File .\build.ps1 test
```

The build helper imports the MSVC environment and uses a workspace-local Rust
toolchain. The release profile enables LTO, symbol stripping, and `panic = "abort"`.

## Project layout

| File | Purpose |
| --- | --- |
| `src/main.rs` | Win32 window, controls, ownerdraw UI, tray, hotkeys, events |
| `src/win.rs` | Raw FFI bindings (no external crates) |
| `src/monitors.rs` | Monitor enumeration, DDC/CI, gamma ramps, WMI fallback |
| `src/night.rs` | Solar times, schedule math, Kelvin→RGB ramp, tests |
| `src/dim.rs` | Time-based auto-dim curve, validation, tests |
| `src/settings.rs` | INI load/save, time parsing, autostart |

## Known limitations

- DDC/CI depends entirely on the monitor and cable. The development/test display here
  (`Generic PnP Monitor`, virtual/remote) exposes no DDC/CI and no WMI panel
  brightness, so hardware controls and auto dim could not be exercised end-to-end on it.
- Gamma-based night light does not affect exclusive-fullscreen games that bypass the
  desktop gamma ramp.
- No per-monitor independent night-light scheduling yet; the ramp is applied to all
  displays.
- In dark mode the small native input fields (times, latitude/longitude, VCP value)
  keep a light face with dark text; the rest of the interface, including buttons and
  sliders, follows the theme.
