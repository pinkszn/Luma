//! Monitor discovery, DDC/CI (MCCS) controls, internal-panel brightness via WMI,
//! and gamma-ramp application.

use crate::win::*;
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr::null_mut;

pub struct PhysicalMonitor {
    pub handle: HANDLE,
    pub description: String,
}

impl Drop for PhysicalMonitor {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() {
                let mut pm = PHYSICAL_MONITOR {
                    hPhysicalMonitor: self.handle,
                    szPhysicalMonitorDescription: [0; 128],
                };
                DestroyPhysicalMonitors(1, &mut pm);
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct BrightnessInfo {
    pub current: u32,
    pub max: u32,
    pub dxva: bool,
}

pub struct Display {
    pub hmonitor: HMONITOR,
    pub device: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub physical: Option<PhysicalMonitor>,
    pub brightness: Option<BrightnessInfo>,
}

impl Display {
    pub fn has_ddc(&self) -> bool {
        self.physical.is_some()
    }

    /// Re-acquire the physical monitor handle. DDC/CI can fail on the first
    /// try (monitor asleep, sleep/wake race, another app holding the handle),
    /// so retry a few times before declaring the display unsupported.
    pub fn reconnect(&mut self) {
        unsafe {
            for attempt in 0..3 {
                if attempt > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(40));
                }
                if self.physical.is_none() {
                    self.physical = get_physical(self.hmonitor);
                }
                if self.physical.is_some() && self.read_brightness().is_some() {
                    return;
                }
                self.physical = None;
            }
        }
    }

    pub fn read_brightness(&mut self) -> Option<BrightnessInfo> {
        let p = self.physical.as_ref()?;
        unsafe {
            let mut min = 0u32;
            let mut cur = 0u32;
            let mut max = 0u32;
            if GetMonitorBrightness(p.handle, &mut min, &mut cur, &mut max) != 0 && max > 0 {
                let info = BrightnessInfo {
                    current: cur.min(max),
                    max,
                    dxva: true,
                };
                self.brightness = Some(info);
                return Some(info);
            }
        }
        if let Some((cur, max)) = p.get_vcp(0x10) {
            let info = BrightnessInfo {
                current: cur,
                max: max.max(1),
                dxva: false,
            };
            self.brightness = Some(info);
            Some(info)
        } else {
            None
        }
    }

    pub fn set_brightness(&mut self, value: u32) -> bool {
        let (dxva, max) = match self.brightness {
            Some(b) => (b.dxva, b.max),
            None => (false, 100),
        };
        let p = match self.physical.as_ref() {
            Some(p) => p,
            None => return false,
        };
        let value = value.min(max);
        let ok = if dxva {
            unsafe { SetMonitorBrightness(p.handle, value) != 0 }
        } else {
            p.set_vcp(0x10, value)
        };
        if ok {
            std::thread::sleep(std::time::Duration::from_millis(40));
            let _ = self.read_brightness();
        }
        ok
    }

    pub fn get_vcp(&self, code: u8) -> Option<(u32, u32)> {
        self.physical.as_ref()?.get_vcp(code)
    }

    pub fn set_vcp(&self, code: u8, value: u32) -> bool {
        self.physical
            .as_ref()
            .map(|p| p.set_vcp(code, value))
            .unwrap_or(false)
    }

    pub fn label(&self) -> String {
        let ddc = if self.physical.is_some() { " · DDC" } else { "" };
        format!("{}  ({}×{}){}", self.name, self.width, self.height, ddc)
    }
}

impl PhysicalMonitor {
    pub fn get_vcp(&self, code: u8) -> Option<(u32, u32)> {
        unsafe {
            let mut ty: i32 = 0;
            let mut cur: u32 = 0;
            let mut max: u32 = 0;
            if GetVCPFeatureAndVCPFeatureReply(self.handle, code, &mut ty, &mut cur, &mut max) != 0 {
                Some((cur, max))
            } else {
                None
            }
        }
    }

    pub fn set_vcp(&self, code: u8, value: u32) -> bool {
        unsafe { SetVCPFeature(self.handle, code, value) != 0 }
    }
}

unsafe extern "system" fn enum_cb(
    hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let list = &mut *(lparam as *mut Vec<Display>);
    let mut mi: MONITORINFOEXW = std::mem::zeroed();
    mi.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut mi) == 0 {
        return 1;
    }
    let device = from_wide(&mi.szDevice);
    let mut physical = get_physical(hmon);
    let mut name = String::new();
    let mut dd: DISPLAY_DEVICEW = std::mem::zeroed();
    dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
    let dev_w = wide(&device);
    if EnumDisplayDevicesW(dev_w.as_ptr(), 0, &mut dd, 0) != 0 {
        name = from_wide(&dd.DeviceString);
    }
    if name.is_empty() || name.contains("Generic") {
        if let Some(p) = physical.as_ref() {
            if !p.description.is_empty() && !p.description.contains("Generic") {
                name = p.description.clone();
            }
        }
    }
    if name.is_empty() {
        name = format!("Display {}", list.len() + 1);
    }
    let width = (mi.rcMonitor.right - mi.rcMonitor.left).max(0) as u32;
    let height = (mi.rcMonitor.bottom - mi.rcMonitor.top).max(0) as u32;
    let mut d = Display {
        hmonitor: hmon,
        device,
        name,
        width,
        height,
        physical: physical.take(),
        brightness: None,
    };
    let _ = d.read_brightness();
    list.push(d);
    1
}

unsafe fn get_physical(hmon: HMONITOR) -> Option<PhysicalMonitor> {
    let mut count: u32 = 0;
    if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) == 0 || count == 0 {
        return None;
    }
    let mut arr = vec![
        PHYSICAL_MONITOR {
            hPhysicalMonitor: null_mut(),
            szPhysicalMonitorDescription: [0; 128],
        };
        count as usize
    ];
    if GetPhysicalMonitorsFromHMONITOR(hmon, count, arr.as_mut_ptr()) == 0 {
        return None;
    }
    if count > 1 {
        DestroyPhysicalMonitors(count - 1, arr[1..].as_mut_ptr());
    }
    let first = arr[0];
    if first.hPhysicalMonitor.is_null() {
        return None;
    }
    Some(PhysicalMonitor {
        handle: first.hPhysicalMonitor,
        description: from_wide(&first.szPhysicalMonitorDescription),
    })
}

pub fn enumerate() -> Vec<Display> {
    let mut list: Vec<Display> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            null_mut(),
            std::ptr::null(),
            enum_cb,
            &mut list as *mut Vec<Display> as LPARAM,
        );
    }
    list
}

pub fn apply_gamma(device: &str, ramp: &[u16; 768]) -> bool {
    unsafe {
        let driver = wide("DISPLAY");
        let dev = wide(device);
        let dc = CreateDCW(driver.as_ptr(), dev.as_ptr(), std::ptr::null(), std::ptr::null());
        if dc.is_null() {
            return false;
        }
        let ok = SetDeviceGammaRamp(dc, ramp.as_ptr() as *const c_void) != 0;
        DeleteDC(dc);
        ok
    }
}

pub fn read_gamma(device: &str) -> Option<[u16; 768]> {
    unsafe {
        let driver = wide("DISPLAY");
        let dev = wide(device);
        let dc = CreateDCW(driver.as_ptr(), dev.as_ptr(), std::ptr::null(), std::ptr::null());
        if dc.is_null() {
            return None;
        }
        let mut ramp = [0u16; 768];
        let ok = GetDeviceGammaRamp(dc, ramp.as_mut_ptr() as *mut c_void) != 0;
        DeleteDC(dc);
        if ok {
            Some(ramp)
        } else {
            None
        }
    }
}

pub fn gamma_looks_applied(device: &str, expected: &[u16; 768]) -> bool {
    // The middle of the ramp is enough to spot a driver reset, with tolerance
    // for driver-side clamping.
    match read_gamma(device) {
        Some(cur) => {
            let idx = [512usize, 384, 640]; // blue mid, green mid, red-ish
            idx.iter()
                .all(|&i| (cur[i] as i32 - expected[i] as i32).abs() <= 1200)
        }
        None => false,
    }
}

const WMI_GET: &str = "$ErrorActionPreference='SilentlyContinue';\
$b=Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -First 1;\
if($b){[Console]::Out.Write([string]$b.CurrentBrightness)}";

pub fn wmi_panel_brightness() -> Option<u32> {
    let out = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            WMI_GET,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    s.trim().parse::<u32>().ok().filter(|v| *v <= 100)
}

pub fn wmi_set_panel_brightness(value: u32) -> bool {
    let script = format!(
        "$ErrorActionPreference='SilentlyContinue';\
         $m=Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods | Select-Object -First 1;\
         if($m){{$m.WmiSetBrightness(1,{});[Console]::Out.Write('ok')}}",
        value.min(100)
    );
    Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn diagnose() {
    let tz = unsafe { current_tz_offset_minutes() };
    println!("Luma diagnostics");
    println!("UTC offset: {} min", tz);
    let displays = enumerate();
    println!("Displays: {}", displays.len());
    for (i, d) in displays.iter().enumerate() {
        println!(
            "  [{}] {} | {} | {}x{} | ddc={}",
            i,
            d.name,
            d.device,
            d.width,
            d.height,
            d.has_ddc()
        );
        if let Some(p) = d.physical.as_ref() {
            println!("      physical: {:?}", p.description);
            for code in [0x10u8, 0x12, 0x60, 0x62, 0x87, 0xD6] {
                match p.get_vcp(code) {
                    Some((cur, max)) => println!("      0x{code:02X}: {cur} / {max}"),
                    None => println!("      0x{code:02X}: unsupported"),
                }
            }
        }
        if !d.has_ddc() {
            unsafe {
                let mut count: u32 = 0;
                let ok = GetNumberOfPhysicalMonitorsFromHMONITOR(d.hmonitor, &mut count);
                println!(
                    "      DDC probe: GetNumberOfPhysicalMonitors -> ok={} count={} err={}",
                    ok,
                    count,
                    GetLastError()
                );
            }
        }
        match d.brightness {
            Some(b) => println!(
                "      brightness: {}/{} via {}",
                b.current,
                b.max,
                if b.dxva { "DXVA2" } else { "VCP 0x10" }
            ),
            None => println!("      brightness: unavailable"),
        }
    }
    match wmi_panel_brightness() {
        Some(v) => println!("WMI panel brightness: {v}"),
        None => println!("WMI panel brightness: not available"),
    }
}
