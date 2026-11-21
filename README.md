# Luma

Luma is a small, dependency-free Windows monitor utility written in Rust. It combines
hardware monitor control (DDC/CI), Windows brightness and gamma control, and a
sunrise/sunset-aware night-light scheduler in one ~355 KB executable.

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

## Features

### Displays tab

- Per-monitor brightness slider (DDC/CI VCP `0x10`) with Windows panel-brightness
  fallback where available.
- Brightness hotkeys: `Ctrl+Alt+Up` / `Ctrl+Alt+Down`.
- VCP control browser: 13 common controls (brightness, contrast, input source, volume,
  color preset, power mode, sharpness, red/green/blue gain, etc.) plus any custom
  `0x00`–`0xFF` VCP code. Read current value, set a value, or enumerate every
  supported code on that monitor.
- Gamma ramp read/write per display.

### Night Light tab

- Modes:
  - **Sun** — sunrise/sunset computed locally from latitude and longitude (NOAA
    algorithm), with configurable offsets before/after sunset and sunrise.
  - **Custom** — separate weekday and weekend on/off windows, including windows that
    cross midnight.
  - **Always** — keep the filter active permanently.
- Color temperature (1000–6500 K, Tanner Helland mapping) and strength (0–100%).
- Fade duration (0–240 min) so the ramp eases in and out instead of snapping.
- Latitude, longitude and UTC offset for the solar calculation, and the weekday and weekend schedules in `HH:MM` form, live on this tab.
- "Pause 1 hour" button; pauses survive restarts.
- Toggle the filter globally with `Ctrl+Alt+N`.
- Live status showing the current mode, next transition, and computed sunrise/sunset.

### Auto dim tab

- Dims the display automatically on a schedule or when idle; "Apply now" saves the current settings.

### Settings tab

- Fade duration, minimize-to-tray, dark mode and run-at-login (per-user registry entry).
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
| `src/main.rs` | Win32 window, controls, tray, hotkeys, event handling |
| `src/win.rs` | Raw FFI bindings (no external crates) |
| `src/monitors.rs` | Monitor enumeration, DDC/CI, gamma ramps, WMI fallback |
| `src/night.rs` | Solar times, schedule math, Kelvin→RGB ramp, tests |
| `src/settings.rs` | INI load/save, time parsing, autostart |

## Known limitations

- DDC/CI depends entirely on the monitor and cable. The development/test display here
  (`Generic PnP Monitor`, virtual/remote) exposes no DDC/CI and no WMI panel
  brightness, so hardware controls could not be exercised on it.
- Gamma-based night light does not affect exclusive-fullscreen games that bypass the
  desktop gamma ramp.
- No per-monitor independent night-light scheduling yet; the ramp is applied to all
  displays.
