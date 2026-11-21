#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod dim;
mod monitors;
mod night;
mod settings;
mod win;

use crate::monitors::{apply_gamma, read_gamma, Display};
use crate::night::{solar_summary, NightInfo};
use crate::settings::{
    config_dir, normalize_time, now_epoch, parse_hhmm, time_minutes, Settings, DEFAULT_FADE,
    DEFAULT_STRENGTH, DEFAULT_TEMP,
};
use crate::win::*;
use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::Duration;

const CLASS_NAME: &str = "LumaMainWindow";
const WINDOW_TITLE: &str = "Luma — Monitor & Night Light";
const CLIENT_W: i32 = 1060;
const CLIENT_H: i32 = 780;
const MUTEX_NAME: &str = "Local\\LumaMonitorSingleton.v3";

// Palette (COLORREF is 0x00BBGGRR).
const fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16)
}

pub struct Palette {
    pub bg: u32,
    pub card: u32,
    pub well: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent_dark: u32,
    pub accent_soft: u32,
    pub accent_ink: u32,
    pub header: u32,
    pub header_sub: u32,
    pub nav_dot: u32,
    pub disabled_bg: u32,
    pub disabled_text: u32,
}

pub const LIGHT_PALETTE: Palette = Palette {
    bg: rgb(0xF4, 0xF5, 0xF7),
    card: rgb(0xFF, 0xFF, 0xFF),
    well: rgb(0xF1, 0xF3, 0xF7),
    border: rgb(0xE3, 0xE6, 0xEC),
    text: rgb(0x20, 0x26, 0x33),
    muted: rgb(0x6C, 0x77, 0x89),
    accent: rgb(0xE0, 0x7B, 0x39),
    accent_dark: rgb(0xB9, 0x5A, 0x1F),
    accent_soft: rgb(0xFB, 0xEE, 0xE4),
    // Ink for text and glyphs drawn on top of the accent fill. The accent is
    // a mid-tone orange, so white only reaches ~2.6:1; near-black is ~5.7:1.
    accent_ink: rgb(0x2B, 0x14, 0x05),
    header: rgb(0x17, 0x1C, 0x26),
    header_sub: rgb(0xAC, 0xB5, 0xC4),
    nav_dot: rgb(0xC3, 0xC9, 0xD4),
    disabled_bg: rgb(0xDD, 0xE1, 0xE8),
    disabled_text: rgb(0x94, 0x9D, 0xAC),
};

pub const DARK_PALETTE: Palette = Palette {
    bg: rgb(0x13, 0x17, 0x1E),
    card: rgb(0x1C, 0x22, 0x2B),
    well: rgb(0x23, 0x2A, 0x35),
    border: rgb(0x30, 0x38, 0x45),
    text: rgb(0xE8, 0xEC, 0xF2),
    muted: rgb(0x98, 0xA3, 0xB3),
    accent: rgb(0xE8, 0x8A, 0x4A),
    accent_dark: rgb(0xC9, 0x6A, 0x2C),
    accent_soft: rgb(0x39, 0x2A, 0x1E),
    accent_ink: rgb(0x2B, 0x14, 0x05),
    header: rgb(0x0B, 0x0E, 0x13),
    header_sub: rgb(0x9A, 0xA5, 0xB3),
    nav_dot: rgb(0x4A, 0x54, 0x64),
    disabled_bg: rgb(0x2A, 0x31, 0x3C),
    disabled_text: rgb(0x7A, 0x84, 0x94),
};

static DARK_MODE: AtomicBool = AtomicBool::new(false);
static VCP_EDIT_OLD_PROC: AtomicIsize = AtomicIsize::new(0);
static COMBO_OLD_PROC: AtomicIsize = AtomicIsize::new(0);
// Explorer broadcasts this when the taskbar restarts; the tray icon must be
// added again or Luma stays hidden with no way back.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn vcp_edit_subclass(
    hwnd: HWND,
    msg: u32,
    w: usize,
    l: isize,
) -> isize {
    if msg == WM_KEYDOWN && w == VK_RETURN as usize {
        let parent = GetParent(hwnd);
        SendMessageW(
            parent,
            WM_COMMAND,
            C_VCP_SET as usize,
            hwnd as isize,
        );
        return 0;
    }
    CallWindowProcW(
        VCP_EDIT_OLD_PROC.load(Ordering::Relaxed),
        hwnd,
        msg,
        w,
        l,
    )
}

/// Combo boxes draw their closed field themselves from `WM_DRAWITEM`, but the
/// drop arrow lives outside that path and comes out as a light system button on
/// a dark card. Paint it in the palette colours after every paint.
unsafe fn draw_combo_arrow(hwnd: HWND, hdc: HDC) {
    if hdc.is_null() {
        return;
    }
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    if rc.right - rc.left < 40 {
        return;
    }
    // Cover the 1px system frame that is left inside the client area.
    let strips = [
        RECT { left: rc.left, top: rc.top, right: rc.right, bottom: rc.top + 2 },
        RECT { left: rc.left, top: rc.bottom - 2, right: rc.right, bottom: rc.bottom },
        RECT { left: rc.left, top: rc.top, right: rc.left + 2, bottom: rc.bottom },
        RECT { left: rc.right - 2, top: rc.top, right: rc.right, bottom: rc.bottom },
    ];
    for s in &strips {
        fill_color(hdc, s, pal().card);
    }
    let frame = RECT {
        left: rc.right - 30,
        top: 1,
        right: rc.right - 1,
        bottom: rc.bottom - 1,
    };
    round_fill(hdc, &frame, 5, pal().well, pal().border);
    let cx = (frame.left + frame.right) / 2;
    let cy = (frame.top + frame.bottom) / 2;
    for row in 0..5 {
        let tri = RECT {
            left: cx - 4 + row,
            top: cy - 2 + row,
            right: cx + 5 - row,
            bottom: cy - 1 + row,
        };
        fill_color(hdc, &tri, pal().muted);
    }
}

unsafe extern "system" fn combo_arrow_subclass(
    hwnd: HWND,
    msg: u32,
    w: usize,
    l: isize,
) -> isize {
    const WM_PRINTCLIENT: u32 = 0x0318;
    if msg == 0x0085 {
        // WM_NCPAINT: the themed frame is a light 1px box on a dark card, and
        // the field already draws its own border.
        return 0;
    }
    let r = CallWindowProcW(
        COMBO_OLD_PROC.load(Ordering::Relaxed),
        hwnd,
        msg,
        w,
        l,
    );
    if msg == WM_PAINT {
        let hdc = GetDC(hwnd);
        draw_combo_arrow(hwnd, hdc);
        ReleaseDC(hwnd, hdc);
    } else if msg == WM_PRINTCLIENT && l != 0 {
        draw_combo_arrow(hwnd, l as HDC);
    }
    r
}

pub fn pal() -> &'static Palette {
    if DARK_MODE.load(Ordering::Relaxed) {
        &DARK_PALETTE
    } else {
        &LIGHT_PALETTE
    }
}

pub fn set_dark_mode(dark: bool) {
    DARK_MODE.store(dark, Ordering::Relaxed);
}

const COL_WHITE: u32 = rgb(0xFF, 0xFF, 0xFF);

const HEADER_H: i32 = 92;
const SIDE_W: i32 = 208;
const CONTENT_X: i32 = 232;
const CONTENT_W: i32 = CLIENT_W - CONTENT_X - 24;

// Sunrise/sunset presets. Offsets are standard time; the "Use system" button
// follows Windows including daylight saving.
const CITIES: &[(&str, f64, f64, f64)] = &[
    ("Amsterdam, Netherlands", 52.3676, 4.9041, 1.0),
    ("Anchorage, USA", 61.2181, -149.9003, -9.0),
    ("Athens, Greece", 37.9838, 23.7275, 2.0),
    ("Auckland, New Zealand", -36.8485, 174.7633, 12.0),
    ("Bangkok, Thailand", 13.7563, 100.5018, 7.0),
    ("Berlin, Germany", 52.5200, 13.4050, 1.0),
    ("Bogotá, Colombia", 4.7110, -74.0721, -5.0),
    ("Brisbane, Australia", -27.4698, 153.0251, 10.0),
    ("Buenos Aires, Argentina", -34.6037, -58.3816, -3.0),
    ("Cairo, Egypt", 30.0444, 31.2357, 2.0),
    ("Cebu, Philippines", 10.3157, 123.8854, 8.0),
    ("Chicago, USA", 41.8781, -87.6298, -6.0),
    ("Denver, USA", 39.7392, -104.9903, -7.0),
    ("Dubai, UAE", 25.2048, 55.2708, 4.0),
    ("Dublin, Ireland", 53.3498, -6.2603, 0.0),
    ("Hanoi, Vietnam", 21.0278, 105.8342, 7.0),
    ("Hong Kong", 22.3193, 114.1694, 8.0),
    ("Honolulu, USA", 21.3069, -157.8583, -10.0),
    ("Istanbul, Türkiye", 41.0082, 28.9784, 3.0),
    ("Jakarta, Indonesia", -6.2088, 106.8456, 7.0),
    ("Johannesburg, South Africa", -26.2041, 28.0473, 2.0),
    ("Kuala Lumpur, Malaysia", 3.1390, 101.6869, 8.0),
    ("Lagos, Nigeria", 6.5244, 3.3792, 1.0),
    ("Lima, Peru", -12.0464, -77.0428, -5.0),
    ("Lisbon, Portugal", 38.7223, -9.1393, 0.0),
    ("London, United Kingdom", 51.5074, -0.1278, 0.0),
    ("Los Angeles, USA", 34.0522, -118.2437, -8.0),
    ("Madrid, Spain", 40.4168, -3.7038, 1.0),
    ("Manila, Philippines", 14.5995, 120.9842, 8.0),
    ("Melbourne, Australia", -37.8136, 144.9631, 10.0),
    ("Mexico City, Mexico", 19.4326, -99.1332, -6.0),
    ("Moscow, Russia", 55.7558, 37.6173, 3.0),
    ("Mumbai, India", 19.0760, 72.8777, 5.5),
    ("Nairobi, Kenya", -1.2921, 36.8219, 3.0),
    ("New Delhi, India", 28.6139, 77.2090, 5.5),
    ("New York, USA", 40.7128, -74.0060, -5.0),
    ("Paris, France", 48.8566, 2.3522, 1.0),
    ("Perth, Australia", -31.9505, 115.8605, 8.0),
    ("Reykjavík, Iceland", 64.1466, -21.9426, 0.0),
    ("Riyadh, Saudi Arabia", 24.7136, 46.6753, 3.0),
    ("Rome, Italy", 41.9028, 12.4964, 1.0),
    ("Santiago, Chile", -33.4489, -70.6693, -4.0),
    ("Seattle, USA", 47.6062, -122.3321, -8.0),
    ("Seoul, South Korea", 37.5665, 126.9780, 9.0),
    ("Shanghai, China", 31.2304, 121.4737, 8.0),
    ("Singapore", 1.3521, 103.8198, 8.0),
    ("Stockholm, Sweden", 59.3293, 18.0686, 1.0),
    ("Sydney, Australia", -33.8688, 151.2093, 10.0),
    ("São Paulo, Brazil", -23.5505, -46.6333, -3.0),
    ("Taipei, Taiwan", 25.0330, 121.5654, 8.0),
    ("Tokyo, Japan", 35.6762, 139.6503, 9.0),
    ("Toronto, Canada", 43.6532, -79.3832, -5.0),
    ("Vancouver, Canada", 49.2827, -123.1207, -8.0),
];

const TZ_MIN: f64 = -12.0;
const TZ_COUNT: usize = 53; // -12:00 .. +14:00 in half-hour steps

fn tz_label(tz: f64) -> String {
    let sign = if tz < 0.0 { '-' } else { '+' };
    let a = tz.abs();
    let h = a.floor() as i32;
    let m = ((a - h as f64) * 60.0).round() as i32;
    format!("UTC{}{:02}:{:02}", sign, h, m)
}

fn tz_selection(tz: f64) -> usize {
    (((tz - TZ_MIN) * 2.0).round() as i64).clamp(0, TZ_COUNT as i64 - 1) as usize
}

/// Nearest preset for a coordinate pair, if it is close enough to be the one
/// the user picked (otherwise the fields are custom).
fn city_selection(lat: f64, lon: f64) -> Option<usize> {
    let mut best = None;
    let mut best_d = f64::MAX;
    for (i, (_, la, lo, _)) in CITIES.iter().enumerate() {
        let d = (la - lat).abs() + (lo - lon).abs();
        if d < best_d {
            best_d = d;
            best = Some(i);
        }
    }
    if best_d <= 0.05 {
        best
    } else {
        None
    }
}

const C_HEADER: u16 = 70;
const C_SIDEBAR: u16 = 71;
const C_NAV0: u16 = 80;
const C_NAV1: u16 = 81;
const C_NAV2: u16 = 82;
const C_NAV3: u16 = 83;

const C_MON_LIST: u16 = 101;
const C_MON_REFRESH: u16 = 102;
const C_BRIGHT: u16 = 103;
const C_BRIGHT_LABEL: u16 = 104;
const C_PANEL: u16 = 105;
const C_PANEL_LABEL: u16 = 106;
const C_PANEL_CAPTION: u16 = 107;
const C_VCP_COMBO: u16 = 108;
const C_VCP_READ: u16 = 109;
const C_VCP_SLIDER: u16 = 110;
const C_VCP_VALUE: u16 = 111;
const C_VCP_MAX: u16 = 112;
const C_VCP_EDIT: u16 = 113;
const C_VCP_SET: u16 = 114;
const C_VCP_ENUM: u16 = 115;
const C_VCP_CUSTOM: u16 = 116;
const C_VCP_USE: u16 = 117;
const C_DISP_STATUS: u16 = 118;
const C_BRIGHT_GROUP: u16 = 119;
const C_VCP_GROUP: u16 = 120;
const C_BRIGHT_CAPTION: u16 = 121;
const C_VCP_CAPTION: u16 = 122;
const C_VCP_HINT: u16 = 123;
const C_MON_GROUP: u16 = 124;

const C_NL_ENABLE: u16 = 201;
const C_NL_MODE: u16 = 202;
const C_NL_STATUS: u16 = 203;
const C_NL_LAT: u16 = 204;
const C_NL_LON: u16 = 205;
const C_NL_TZ: u16 = 206;
const C_NL_SUN: u16 = 207;
const C_NL_WD_START: u16 = 208;
const C_NL_WD_END: u16 = 209;
const C_NL_WEEKEND: u16 = 210;
const C_NL_WE_START: u16 = 211;
const C_NL_WE_END: u16 = 212;
const C_NL_TEMP: u16 = 213;
const C_NL_TEMP_LABEL: u16 = 214;
const C_NL_STRENGTH: u16 = 215;
const C_NL_STRENGTH_LABEL: u16 = 216;
const C_NL_FADE: u16 = 217;
const C_NL_FADE_LABEL: u16 = 218;
const C_NL_PAUSE: u16 = 219;
const C_NL_APPLY: u16 = 220;
const C_NL_RESET_TINT: u16 = 226;
const C_NL_PREVIEW: u16 = 227;
const C_NL_INFO: u16 = 221;
const C_NL_MODE_LABEL: u16 = 222;
const C_NL_SOLAR_GROUP: u16 = 223;
const C_NL_CUSTOM_GROUP: u16 = 224;
const C_NL_TUNING_GROUP: u16 = 225;
const C_NL_LOCATION: u16 = 228;
const C_NL_USE_TZ: u16 = 229;

const C_AUTOSTART: u16 = 301;
const C_MINIMIZE: u16 = 302;
const C_OPEN_FOLDER: u16 = 303;
const C_RESET: u16 = 304;
const C_ABOUT: u16 = 305;
const C_HOTKEYS: u16 = 306;
const C_START_GROUP: u16 = 307;
const C_HOTKEY_GROUP: u16 = 308;
const C_ABOUT_GROUP: u16 = 309;
const C_SETTINGS_HINT: u16 = 310;
const C_DARK_MODE: u16 = 311;

const C_DIM_ENABLE: u16 = 401;
const C_DIM_EVENING: u16 = 402;
const C_DIM_NIGHT: u16 = 403;
const C_DIM_DAY: u16 = 404;
const C_DIM_EVENING_PCT: u16 = 405;
const C_DIM_NIGHT_PCT: u16 = 406;
const C_DIM_FADE: u16 = 407;
const C_DIM_TARGET: u16 = 408;
const C_DIM_PHASE: u16 = 409;
const C_DIM_APPLY: u16 = 410;
const C_DIM_GROUP_A: u16 = 411;
const C_DIM_GROUP_B: u16 = 412;
const C_DIM_GROUP_C: u16 = 413;
const C_DIM_STATUS: u16 = 414;
const C_DIM_CHART: u16 = 415;
const C_DIM_GAUGE: u16 = 416;
const C_DIM_DAY_PCT: u16 = 417;

const C_FOOTER: u16 = 901;

const TRAY_SHOW: usize = 4001;
const TRAY_TOGGLE: usize = 4002;
const TRAY_PAUSE: usize = 4003;
const TRAY_EXIT: usize = 4004;

const VCP_CODES: [(u8, &str); 13] = [
    (0x10, "Brightness"),
    (0x12, "Contrast"),
    (0x14, "Color preset"),
    (0x16, "Red gain"),
    (0x17, "Green gain"),
    (0x18, "Blue gain"),
    (0x60, "Input source"),
    (0x62, "Audio volume"),
    (0x72, "Gamma"),
    (0x87, "Sharpness"),
    (0x8A, "Saturation"),
    (0x8B, "Hue"),
    (0xD6, "Power mode"),
];
const VCP_CUSTOM: usize = VCP_CODES.len();

#[derive(Default)]
struct Controls {
    list: HWND,
    mon_group: HWND,
    refresh: HWND,
    bright_group: HWND,
    bright_caption: HWND,
    bright: HWND,
    bright_label: HWND,
    panel_caption: HWND,
    panel: HWND,
    panel_label: HWND,
    vcp_group: HWND,
    vcp_caption: HWND,
    vcp_combo: HWND,
    vcp_read: HWND,
    vcp_slider: HWND,
    vcp_value: HWND,
    vcp_max: HWND,
    vcp_edit: HWND,
    vcp_set: HWND,
    vcp_enum: HWND,
    vcp_custom: HWND,
    vcp_use: HWND,
    vcp_hint: HWND,
    disp_status: HWND,

    nl_enable: HWND,
    nl_mode: HWND,
    nl_status: HWND,
    nl_location: HWND,
    use_tz: HWND,
    lat: HWND,
    lon: HWND,
    tz: HWND,
    sun_info: HWND,
    wd_start: HWND,
    wd_end: HWND,
    weekend: HWND,
    we_start: HWND,
    we_end: HWND,
    temp: HWND,
    temp_label: HWND,
    strength: HWND,
    strength_label: HWND,
    fade: HWND,
    fade_label: HWND,
    pause: HWND,
    apply: HWND,
    tuning_group: HWND,
    reset_tint: HWND,
    tint_preview: HWND,
    nl_info: HWND,

    dim_enable: HWND,
    dim_evening: HWND,
    dim_night: HWND,
    dim_day: HWND,
    dim_day_pct: HWND,
    dim_evening_pct: HWND,
    dim_night_pct: HWND,
    dim_fade: HWND,
    dim_target: HWND,
    dim_phase: HWND,
    dim_apply: HWND,
    dim_status: HWND,
    dim_chart: HWND,
    dim_gauge: HWND,

    autostart: HWND,
    minimize: HWND,
    dark_mode: HWND,
    open_folder: HWND,
    reset: HWND,
    about: HWND,
    hotkeys: HWND,
    settings_hint: HWND,

    footer: HWND,
}

#[derive(Default)]
struct Fonts {
    base: HFONT,
    bold: HFONT,
    title: HFONT,
    heading: HFONT,
    small: HFONT,
}

struct App {
    hwnd: HWND,
    hinst: HINSTANCE,
    mutex: HANDLE,
    fonts: Fonts,
    icon: HICON,
    controls: Controls,
    pages: [Vec<HWND>; 4],
    edits: Vec<HWND>,
    page: usize,
    br_bg: HBRUSH,
    br_card: HBRUSH,
    br_well: HBRUSH,

    settings: Settings,
    displays: Vec<Display>,
    mon_labels: Vec<String>,
    selected: usize,
    panel_brightness: Option<u32>,
    panel_checked: bool,

    vcp_code: u8,
    vcp_max: u32,
    vcp_enum_visible: bool,

    original_gamma: Vec<(String, [u16; 768])>,
    applied_any: bool,
    last_blend: f32,
    last_apply: u64,
    started: u64,

    dirty: bool,
    dirty_since: u64,
    last_nl_status: String,
    last_footer: String,
    dim_last_pct: Option<u8>,
    dim_last_apply: u64,
    last_manual_bright: u64,
    dim_last_status: String,
    tray: bool,
}

impl App {
    fn new(hinst: HINSTANCE, mutex: HANDLE) -> Self {
        let is_new = !settings::config_path().exists();
        let mut settings = Settings::load();
        if is_new {
            settings.tz = unsafe { current_tz_offset_minutes() } as f64 / 60.0;
        }
        let selected = settings.selected;
        Self {
            hwnd: null_mut(),
            hinst,
            mutex,
            fonts: Fonts::default(),
            icon: null_mut(),
            controls: Controls::default(),
            pages: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            edits: Vec::new(),
            page: 0,
            br_bg: null_mut(),
            br_card: null_mut(),
            br_well: null_mut(),
            settings,
            displays: Vec::new(),
            mon_labels: Vec::new(),
            selected,
            panel_brightness: None,
            panel_checked: false,
            vcp_code: 0x10,
            vcp_max: 100,
            vcp_enum_visible: false,
            original_gamma: Vec::new(),
            applied_any: false,
            last_blend: 0.0,
            last_apply: 0,
            started: unsafe { GetTickCount64() },
            dirty: false,
            dirty_since: 0,
            last_nl_status: String::new(),
            last_footer: String::new(),
            dim_last_pct: None,
            dim_last_apply: 0,
            last_manual_bright: 0,
            dim_last_status: String::new(),
            tray: false,
        }
    }

    // ---------- UI construction ----------

    unsafe fn on_create(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        self.log_tray("on_create begin");
        self.hinst = GetModuleHandleW(std::ptr::null());
        set_dark_mode(self.settings.dark);
        self.br_bg = CreateSolidBrush(pal().bg);
        self.br_card = CreateSolidBrush(pal().card);
        self.br_well = CreateSolidBrush(pal().well);
        self.fonts.base = make_font(-15, 400, "Segoe UI");
        self.fonts.bold = make_font(-15, 600, "Segoe UI Semibold");
        self.fonts.title = make_font(-30, 600, "Segoe UI Semibold");
        self.fonts.heading = make_font(-22, 600, "Segoe UI Semibold");
        self.fonts.small = make_font(-13, 400, "Segoe UI");
        self.icon = make_icon(self.hinst);
        SendMessageW(hwnd, WM_SETICON, 0, self.icon as isize);
        SendMessageW(hwnd, WM_SETICON, 1, self.icon as isize);

        self.create_header();
        self.create_sidebar();
        self.create_displays_page();
        self.create_night_page();
        self.create_dim_page();
        self.create_settings_page();
        self.create_footer();
        self.set_page(0);

        self.populate_settings_ui();

        // Register the tray icon before the blocking DDC/CI monitor scan so
        // it appears immediately instead of ~41 s after startup.
        self.add_tray();

        self.refresh_monitors();
        self.restyle_edits();
        self.refresh_night_now();
        RegisterHotKey(hwnd, 1, MOD_CONTROL | MOD_ALT, VK_N);
        RegisterHotKey(hwnd, 2, MOD_CONTROL | MOD_ALT, VK_UP);
        RegisterHotKey(hwnd, 3, MOD_CONTROL | MOD_ALT, VK_DOWN);
        SetTimer(hwnd, 1, 1000, std::ptr::null());
    }

    unsafe fn style_edit(&self, h: HWND) {
        if h.is_null() {
            return;
        }
        SendMessageW(h, EM_SETBKGNDCOLOR, 0, pal().well as isize);
        let mut cf: CHARFORMATW = std::mem::zeroed();
        cf.cbSize = std::mem::size_of::<CHARFORMATW>() as u32;
        cf.dwMask = CFM_COLOR;
        cf.crTextColor = pal().text;
        SendMessageW(
            h,
            EM_SETCHARFORMAT,
            SCF_ALL | SCF_DEFAULT,
            &mut cf as *mut CHARFORMATW as isize,
        );
        InvalidateRect(h, std::ptr::null(), 1);
    }

    unsafe fn restyle_edits(&self) {
        for &h in &self.edits {
            self.style_edit(h);
        }
    }

    unsafe fn apply_theme(&mut self) {
        set_dark_mode(self.settings.dark);
        if !self.br_bg.is_null() {
            DeleteObject(self.br_bg as HANDLE);
        }
        if !self.br_card.is_null() {
            DeleteObject(self.br_card as HANDLE);
        }
        if !self.br_well.is_null() {
            DeleteObject(self.br_well as HANDLE);
        }
        self.br_bg = CreateSolidBrush(pal().bg);
        self.br_card = CreateSolidBrush(pal().card);
        self.br_well = CreateSolidBrush(pal().well);
        self.restyle_edits();
        RedrawWindow(
            self.hwnd,
            std::ptr::null(),
            null_mut(),
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
        );
        if !self.controls.dark_mode.is_null() {
            InvalidateRect(self.controls.dark_mode, std::ptr::null(), 1);
        }
    }

    unsafe fn ctl(
        &mut self,
        class: &str,
        text: &str,
        style: u32,
        ex: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: u16,
        page: usize,
    ) -> HWND {
        let cls = wide(class);
        let txt = wide(text);
        // Combo boxes and list boxes are owner-drawn so their faces, selection
        // bars and dropdown items use the theme colours instead of the system
        // (white in dark mode) defaults.
        let style = match class {
            "COMBOBOX" => style | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS,
            "LISTBOX" => style | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS,
            _ => style,
        };
        let c = CreateWindowExW(
            ex,
            cls.as_ptr(),
            txt.as_ptr(),
            style,
            x,
            y,
            w,
            h,
            self.hwnd,
            id as usize as HMENU,
            self.hinst,
            null_mut(),
        );
        if !c.is_null() {
            if class == "RICHEDIT50W" {
                self.edits.push(c);
                self.style_edit(c);
            } else if matches!(class, "COMBOBOX" | "LISTBOX") {
                // The border and drop arrow of a combo ignore WM_CTLCOLOR*, so
                // pick a theme that matches the palette instead of a white field.
                let theme = if DARK_MODE.load(Ordering::Relaxed) {
                    wide("DarkMode_Explorer")
                } else {
                    wide(" ")
                };
                SetWindowTheme(c, theme.as_ptr(), theme.as_ptr());
                if class == "COMBOBOX" {
                    // -1 is the closed (static) portion, 0 the dropdown rows.
                    SendMessageW(c, CB_SETITEMHEIGHT, 0, 24);
                    SendMessageW(c, CB_SETITEMHEIGHT, usize::MAX, 24);
                    let old_proc = SetWindowLongPtrW(
                        c,
                        GWLP_WNDPROC,
                        combo_arrow_subclass as usize as isize,
                    );
                    COMBO_OLD_PROC.store(old_proc, Ordering::Relaxed);
                } else {
                    SendMessageW(c, LB_SETITEMHEIGHT, 0, 24);
                }
            }
            SendMessageW(c, WM_SETFONT, self.fonts.base as usize, 1);
            if page > 0 && page <= self.pages.len() {
                self.pages[page - 1].push(c);
            }
        }
        c
    }

    unsafe fn label(&mut self, text: &str, x: i32, y: i32, w: i32, h: i32, page: usize) -> HWND {
        self.ctl(
            "STATIC",
            text,
            WS_CHILD | WS_VISIBLE | SS_LEFT | SS_NOPREFIX,
            0,
            x,
            y,
            w,
            h,
            0,
            page,
        )
    }

    unsafe fn value_label(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        page: usize,
    ) -> HWND {
        let hwnd = self.ctl(
            "STATIC",
            text,
            WS_CHILD | WS_VISIBLE | SS_RIGHT | SS_NOPREFIX,
            0,
            x,
            y,
            w,
            h,
            0,
            page,
        );
        SendMessageW(hwnd, WM_SETFONT, self.fonts.bold as usize, 1);
        hwnd
    }

    unsafe fn hint(&mut self, text: &str, x: i32, y: i32, w: i32, h: i32, page: usize) -> HWND {
        let hwnd = self.label(text, x, y, w, h, page);
        SendMessageW(hwnd, WM_SETFONT, self.fonts.small as usize, 1);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 1);
        hwnd
    }

    unsafe fn title(&mut self, text: &str, x: i32, y: i32, page: usize) -> HWND {
        let hwnd = self.label(text, x, y, 500, 34, page);
        SendMessageW(hwnd, WM_SETFONT, self.fonts.heading as usize, 1);
        hwnd
    }

    unsafe fn card(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: u16,
        page: usize,
    ) -> HWND {
        self.ctl(
            "STATIC",
            text,
            WS_CHILD | WS_VISIBLE | SS_OWNERDRAW,
            0,
            x,
            y,
            w,
            h,
            id,
            page,
        )
    }

    unsafe fn button(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: u16,
        primary: bool,
        page: usize,
    ) -> HWND {
        let style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW;
        let _ = primary;
        self.ctl("BUTTON", text, style, 0, x, y, w, h, id, page)
    }

    unsafe fn checkbox(
        &mut self,
        text: &str,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        id: u16,
        page: usize,
    ) -> HWND {
        let style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW;
        let c = self.ctl("BUTTON", text, style, 0, x, y, w, h, id, page);
        SendMessageW(c, WM_SETFONT, self.fonts.base as usize, 1);
        c
    }

    unsafe fn create_header(&mut self) {
        self.ctl(
            "STATIC", "", WS_CHILD | WS_VISIBLE | SS_OWNERDRAW, 0,
            0, 0, CLIENT_W, HEADER_H, C_HEADER, 0,
        );
    }

    unsafe fn create_sidebar(&mut self) {
        self.ctl(
            "STATIC", "", WS_CHILD | WS_VISIBLE | SS_OWNERDRAW, 0,
            0, HEADER_H, SIDE_W, CLIENT_H - HEADER_H, C_SIDEBAR, 0,
        );
        let items = [
            ("Displays", C_NAV0),
            ("Night light", C_NAV1),
            ("Auto dim", C_NAV3),
            ("Settings", C_NAV2),
        ];
        for (i, (text, id)) in items.iter().enumerate() {
            let b = self.ctl(
                "BUTTON",
                text,
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW,
                0,
                16,
                HEADER_H + 26 + i as i32 * 52,
                SIDE_W - 32,
                44,
                *id,
                0,
            );
            SendMessageW(b, WM_SETFONT, self.fonts.bold as usize, 1);
        }
    }

    unsafe fn create_displays_page(&mut self) {
        self.title("Displays", CONTENT_X, 100, 1);
        self.hint(
            "Pick a monitor, set its brightness, or read and write any DDC/CI control it exposes.",
            CONTENT_X + 2,
            136,
            CONTENT_W,
            22,
            1,
        );

        self.controls.mon_group = self.card(
            "Monitors",
            CONTENT_X,
            172,
            256,
            496,
            C_MON_GROUP,
            1,
        );
        self.controls.list = self.ctl(
            "LISTBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | WS_TABSTOP | LBS_NOTIFY | LBS_NOINTEGRALHEIGHT,
            0,
            CONTENT_X + 16,
            212,
            224,
            336,
            C_MON_LIST,
            1,
        );
        SendMessageW(self.controls.list, WM_SETFONT, self.fonts.base as usize, 1);
        self.controls.refresh =
            self.button("Refresh displays", CONTENT_X + 16, 558, 224, 30, C_MON_REFRESH, false, 1);
        self.hint(
            "DDC/CI must be enabled in the monitor's OSD. Laptop panels use the Windows panel-brightness API instead.",
            CONTENT_X + 16,
            598,
            224,
            66,
            1,
        );

        self.controls.bright_group =
            self.card("Brightness", CONTENT_X + 272, 172, 532, 204, C_BRIGHT_GROUP, 1);
        self.controls.bright_caption =
            self.label("Hardware brightness", CONTENT_X + 288, 210, 240, 20, 1);
        self.controls.bright_label =
            self.value_label("—", CONTENT_X + 664, 210, 124, 20, 1);
        SetWindowLongPtrW(self.controls.bright_label, GWLP_USERDATA, 3);
        self.controls.bright = self.ctl(
            SLIDER_CLASS,
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | TBS_AUTOTICKS,
            0,
            CONTENT_X + 288,
            234,
            500,
            32,
            C_BRIGHT,
            1,
        );
        self.controls.panel_caption =
            self.label("Panel brightness", CONTENT_X + 288, 272, 240, 20, 1);
        self.controls.panel_label =
            self.value_label("—", CONTENT_X + 664, 272, 124, 20, 1);
        SetWindowLongPtrW(self.controls.panel_label, GWLP_USERDATA, 3);
        self.controls.panel = self.ctl(
            SLIDER_CLASS,
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | TBS_AUTOTICKS,
            0,
            CONTENT_X + 288,
            296,
            500,
            32,
            C_PANEL,
            1,
        );
        set_slider_range(self.controls.panel, 0, 100);
        set_slider(self.controls.panel, 50);
        self.hint(
            "Hardware uses DDC/CI. Panel brightness is a fallback for laptop or virtual panels that do not answer DDC/CI.",
            CONTENT_X + 288,
            334,
            500,
            36,
            1,
        );

        self.controls.vcp_group =
            self.card("Monitor control", CONTENT_X + 272, 392, 532, 268, C_VCP_GROUP, 1);
        self.controls.vcp_caption =
            self.label("Control", CONTENT_X + 288, 426, 64, 20, 1);
        self.controls.vcp_combo = self.ctl(
            "COMBOBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST,
            0,
            CONTENT_X + 358,
            422,
            310,
            260,
            C_VCP_COMBO,
            1,
        );
        for (_, name) in VCP_CODES {
            add_combo_string(self.controls.vcp_combo, name);
        }
        add_combo_string(self.controls.vcp_combo, "Custom code…");
        set_combo_selection(self.controls.vcp_combo, 0);
        self.controls.vcp_read =
            self.button("Read", CONTENT_X + 680, 422, 92, 28, C_VCP_READ, false, 1);
        self.controls.vcp_slider = self.ctl(
            SLIDER_CLASS,
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            0,
            CONTENT_X + 288,
            458,
            500,
            30,
            C_VCP_SLIDER,
            1,
        );
        self.controls.vcp_max = self.label("", CONTENT_X + 288, 490, 300, 18, 1);
        SetWindowLongPtrW(self.controls.vcp_max, GWLP_USERDATA, 4);
        self.controls.vcp_value =
            self.value_label("", CONTENT_X + 588, 490, 200, 18, 1);
        SetWindowLongPtrW(self.controls.vcp_value, GWLP_USERDATA, 3);
        self.label("Set value", CONTENT_X + 288, 522, 72, 20, 1);
        self.controls.vcp_edit = self.ctl(
            "RICHEDIT50W",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
            0,
            CONTENT_X + 358,
            518,
            90,
            26,
            C_VCP_EDIT,
            1,
        );
        let old_proc = SetWindowLongPtrW(
            self.controls.vcp_edit,
            GWLP_WNDPROC,
            vcp_edit_subclass as usize as isize,
        );
        VCP_EDIT_OLD_PROC.store(old_proc, Ordering::Relaxed);
        self.controls.vcp_set =
            self.button("Set", CONTENT_X + 458, 518, 64, 28, C_VCP_SET, true, 1);
        self.controls.vcp_enum = self.ctl(
            "COMBOBOX",
            "",
            WS_CHILD | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST,
            0,
            CONTENT_X + 544,
            518,
            228,
            220,
            C_VCP_ENUM,
            1,
        );
        self.label("Custom code", CONTENT_X + 288, 556, 92, 20, 1);
        self.controls.vcp_custom = self.ctl(
            "RICHEDIT50W",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
            0,
            CONTENT_X + 382,
            552,
            70,
            26,
            C_VCP_CUSTOM,
            1,
        );
        self.controls.vcp_use =
            self.button("Use", CONTENT_X + 460, 552, 54, 28, C_VCP_USE, false, 1);
        self.hint(
            "Hex, e.g. 0x10",
            CONTENT_X + 522,
            556,
            240,
            20,
            1,
        );
        self.controls.vcp_hint = self.hint(
            "Writes go over DDC/CI. Input 0x60, power 0xD6 and color preset 0x14 get named choices.",
            CONTENT_X + 288,
            588,
            500,
            42,
            1,
        );
        self.controls.disp_status =
            self.label("", CONTENT_X, 676, CONTENT_W, 44, 1);
        SendMessageW(self.controls.disp_status, WM_SETFONT, self.fonts.small as usize, 1);
        SetWindowLongPtrW(self.controls.disp_status, GWLP_USERDATA, 2);
    }

    unsafe fn create_night_page(&mut self) {
        self.title("Night light", CONTENT_X, 100, 2);
        self.controls.nl_status =
            self.label("", CONTENT_X + 2, 136, CONTENT_W - 290, 34, 2);
        SendMessageW(self.controls.nl_status, WM_SETFONT, self.fonts.base as usize, 1);
        SetWindowLongPtrW(self.controls.nl_status, GWLP_USERDATA, 2);
        self.controls.nl_enable = self.checkbox(
            "Night light enabled",
            CONTENT_X + 540,
            106,
            264,
            24,
            C_NL_ENABLE,
            2,
        );

        self.card("Schedule", CONTENT_X, 176, 392, 336, C_NL_CUSTOM_GROUP, 2);
        self.label("Mode", CONTENT_X + 16, 212, 240, 20, 2);
        self.controls.nl_mode = self.ctl(
            "COMBOBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST,
            0,
            CONTENT_X + 16,
            234,
            360,
            200,
            C_NL_MODE,
            2,
        );
        for s in ["Sunset to sunrise", "Custom times", "Always on"] {
            add_combo_string(self.controls.nl_mode, s);
        }
        self.label("Weekday", CONTENT_X + 16, 272, 70, 20, 2);
        self.controls.wd_start = self.ctl(
            "RICHEDIT50W", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL, 0,
            CONTENT_X + 16, 294, 86, 26, C_NL_WD_START, 2);
        self.label("to", CONTENT_X + 110, 298, 20, 20, 2);
        self.controls.wd_end = self.ctl(
            "RICHEDIT50W", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL, 0,
            CONTENT_X + 136, 294, 86, 26, C_NL_WD_END, 2);
        self.controls.weekend = self.checkbox(
            "Different window on weekends",
            CONTENT_X + 16,
            332,
            360,
            24,
            C_NL_WEEKEND,
            2,
        );
        self.label("Weekend", CONTENT_X + 16, 368, 70, 20, 2);
        self.controls.we_start = self.ctl(
            "RICHEDIT50W", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL, 0,
            CONTENT_X + 16, 390, 86, 26, C_NL_WE_START, 2);
        self.label("to", CONTENT_X + 110, 394, 20, 20, 2);
        self.controls.we_end = self.ctl(
            "RICHEDIT50W", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL, 0,
            CONTENT_X + 136, 390, 86, 26, C_NL_WE_END, 2);
        self.hint(
            "24-hour HH:MM. Windows that cross midnight are supported. Solar mode recomputes sunset and sunrise for your coordinates every day.",
            CONTENT_X + 16,
            428,
            360,
            72,
            2,
        );

        self.card("Solar position", CONTENT_X + 408, 176, 396, 208, C_NL_SOLAR_GROUP, 2);
        self.label("Location", CONTENT_X + 424, 212, 364, 18, 2);
        self.controls.nl_location = self.ctl(
            "COMBOBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST,
            0,
            CONTENT_X + 424,
            230,
            364,
            260,
            C_NL_LOCATION,
            2,
        );
        for (name, _, _, _) in CITIES {
            add_combo_string(self.controls.nl_location, name);
        }
        add_combo_string(self.controls.nl_location, "Custom coordinates");
        self.label("Time zone", CONTENT_X + 424, 262, 200, 18, 2);
        self.controls.tz = self.ctl(
            "COMBOBOX",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST,
            0,
            CONTENT_X + 424,
            280,
            250,
            260,
            C_NL_TZ,
            2,
        );
        for i in 0..TZ_COUNT {
            add_combo_string(self.controls.tz, &tz_label(TZ_MIN + i as f64 * 0.5));
        }
        self.controls.use_tz = self.button(
            "Use system",
            CONTENT_X + 680,
            280,
            108,
            26,
            C_NL_USE_TZ,
            false,
            2,
        );
        self.label("Lat", CONTENT_X + 424, 316, 22, 20, 2);
        self.controls.lat = self.ctl(
            "RICHEDIT50W", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL, 0,
            CONTENT_X + 450, 314, 84, 24, C_NL_LAT, 2);
        self.label("Lon", CONTENT_X + 544, 316, 26, 20, 2);
        self.controls.lon = self.ctl(
            "RICHEDIT50W", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL, 0,
            CONTENT_X + 574, 314, 84, 24, C_NL_LON, 2);
        self.controls.sun_info =
            self.label("", CONTENT_X + 424, 344, 364, 34, 2);
        SendMessageW(self.controls.sun_info, WM_SETFONT, self.fonts.small as usize, 1);
        SetWindowLongPtrW(self.controls.sun_info, GWLP_USERDATA, 4);

        self.controls.tuning_group = self.card(
            "Tint & transition",
            CONTENT_X + 408,
            400,
            396,
            296,
            C_NL_TUNING_GROUP,
            2,
        );
        self.controls.reset_tint = self.button(
            "Defaults",
            CONTENT_X + 408 + 276,
            406,
            108,
            26,
            C_NL_RESET_TINT,
            false,
            2,
        );
        self.label("Temperature", CONTENT_X + 424, 434, 200, 20, 2);
        self.controls.temp = self.ctl(
            SLIDER_CLASS, "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | TBS_AUTOTICKS, 0,
            CONTENT_X + 424, 456, 364, 30, C_NL_TEMP, 2);
        self.controls.temp_label =
            self.value_label("—", CONTENT_X + 668, 434, 120, 20, 2);
        SetWindowLongPtrW(self.controls.temp_label, GWLP_USERDATA, 3);
        self.label("Strength", CONTENT_X + 424, 492, 200, 20, 2);
        self.controls.strength = self.ctl(
            SLIDER_CLASS, "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | TBS_AUTOTICKS, 0,
            CONTENT_X + 424, 514, 364, 30, C_NL_STRENGTH, 2);
        self.controls.strength_label =
            self.value_label("—", CONTENT_X + 668, 492, 120, 20, 2);
        SetWindowLongPtrW(self.controls.strength_label, GWLP_USERDATA, 3);
        self.label("Fade", CONTENT_X + 424, 550, 200, 20, 2);
        self.controls.fade = self.ctl(
            SLIDER_CLASS, "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | TBS_AUTOTICKS, 0,
            CONTENT_X + 424, 572, 364, 30, C_NL_FADE, 2);
        self.controls.fade_label =
            self.value_label("—", CONTENT_X + 668, 550, 120, 20, 2);
        SetWindowLongPtrW(self.controls.fade_label, GWLP_USERDATA, 3);
        self.controls.pause =
            self.button("Pause 1 hour", CONTENT_X + 424, 612, 150, 30, C_NL_PAUSE, false, 2);
        self.controls.apply =
            self.button("Apply schedule", CONTENT_X + 586, 612, 202, 30, C_NL_APPLY, true, 2);
        self.controls.tint_preview = self.ctl(
            "STATIC", "", WS_CHILD | WS_VISIBLE | SS_OWNERDRAW, 0,
            CONTENT_X + 424, 644, 364, 36, C_NL_PREVIEW, 2);
        self.controls.nl_info = self.hint(
            "Luma applies its own gamma-ramp tint, so fades are seamless and per-display. It does not change the Windows Night light setting. Turn Windows Night light off while Luma tints, otherwise both tints stack.\nTimes follow the time zone you pick; press Use system to follow Windows, including daylight saving.",
            CONTENT_X,
            528,
            392,
            140,
            2,
        );
    }

    unsafe fn create_dim_page(&mut self) {
        self.title("Auto dim", CONTENT_X, 100, 3);
        self.hint(
            "Lower hardware brightness on a daily curve with gentle fades, so the screen gets dimmer as the evening goes on.",
            CONTENT_X + 2,
            136,
            CONTENT_W - 240,
            40,
            3,
        );
        self.controls.dim_enable = self.checkbox(
            "Enable auto dim",
            CONTENT_X + 600,
            106,
            180,
            24,
            C_DIM_ENABLE,
            3,
        );
        SendMessageW(self.controls.dim_enable, WM_SETFONT, self.fonts.bold as usize, 1);

        self.card("Daily curve", CONTENT_X, 184, 480, 452, C_DIM_GROUP_A, 3);
        self.controls.dim_chart = self.ctl(
            "STATIC", "", WS_CHILD | WS_VISIBLE | SS_OWNERDRAW, 0,
            CONTENT_X + 16, 216, 448, 128, C_DIM_CHART, 3);
        let rows: [(&str, u16, i32, &str, i32); 7] = [
            ("Evening starts", C_DIM_EVENING, 348, "HH:MM", 92),
            ("Evening level", C_DIM_EVENING_PCT, 380, "%", 92),
            ("Night starts", C_DIM_NIGHT, 412, "HH:MM", 92),
            ("Night level", C_DIM_NIGHT_PCT, 444, "%", 92),
            ("Back to day", C_DIM_DAY, 476, "HH:MM", 92),
            ("Day level", C_DIM_DAY_PCT, 508, "%", 92),
            ("Fade", C_DIM_FADE, 540, "min", 70),
        ];
        for (text, id, y, unit, w) in rows {
            self.label(text, CONTENT_X + 16, y + 3, 160, 20, 3);
            let edit = self.ctl(
                "RICHEDIT50W",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL,
                0,
                CONTENT_X + 180,
                y,
                w,
                26,
                id,
                3,
            );
            match id {
                C_DIM_EVENING => self.controls.dim_evening = edit,
                C_DIM_NIGHT => self.controls.dim_night = edit,
                C_DIM_DAY => self.controls.dim_day = edit,
                C_DIM_DAY_PCT => self.controls.dim_day_pct = edit,
                C_DIM_EVENING_PCT => self.controls.dim_evening_pct = edit,
                C_DIM_NIGHT_PCT => self.controls.dim_night_pct = edit,
                _ => self.controls.dim_fade = edit,
            }
            self.hint(unit, CONTENT_X + 280, y + 3, 60, 20, 3);
        }
        self.hint(
            "Ramps are linear, so brightness changes by a fraction of a percent each minute.\nTimes are local and the day anchor is 'Back to day'.",
            CONTENT_X + 16,
            578,
            448,
            56,
            3,
        );

        self.card("Right now", CONTENT_X + 496, 184, 308, 452, C_DIM_GROUP_B, 3);
        self.controls.dim_gauge = self.ctl(
            "STATIC", "", WS_CHILD | WS_VISIBLE | SS_OWNERDRAW, 0,
            CONTENT_X + 512, 224, 276, 10, C_DIM_GAUGE, 3);
        self.controls.dim_target = self.value_label("—", CONTENT_X + 512, 240, 276, 52, 3);
        SendMessageW(self.controls.dim_target, WM_SETFONT, self.fonts.title as usize, 1);
        SetWindowLongPtrW(self.controls.dim_target, GWLP_USERDATA, 3);
        self.controls.dim_phase =
            self.label("", CONTENT_X + 512, 296, 276, 42, 3);
        SendMessageW(self.controls.dim_phase, WM_SETFONT, self.fonts.small as usize, 1);
        SetWindowLongPtrW(self.controls.dim_phase, GWLP_USERDATA, 4);
        self.controls.dim_apply =
            self.button("Apply now", CONTENT_X + 512, 346, 140, 30, C_DIM_APPLY, true, 3);
        self.controls.dim_status = self.hint(
            "Auto dim writes brightness over DDC/CI. Displays without DDC/CI are skipped.",
            CONTENT_X + 512,
            392,
            276,
            140,
            3,
        );

        self.card("Notes", CONTENT_X, 652, 804, 84, C_DIM_GROUP_C, 3);
        self.hint(
            "Manual brightness changes pause auto dim for five minutes, so a slider drag is never fought.\nDaytime brightness is the day level. Enable auto dim to apply the curve.",
            CONTENT_X + 16,
            686,
            772,
            44,
            3,
        );
    }

    unsafe fn create_settings_page(&mut self) {
        self.title("Settings", CONTENT_X, 100, 4);
        self.hint(
            "Startup, hotkeys and where Luma keeps your preferences.",
            CONTENT_X + 2,
            136,
            CONTENT_W,
            22,
            4,
        );

        self.card("Startup", CONTENT_X, 176, 500, 170, C_START_GROUP, 4);
        self.controls.autostart = self.checkbox(
            "Start Luma with Windows",
            CONTENT_X + 16,
            216,
            468,
            24,
            C_AUTOSTART,
            4,
        );
        self.controls.minimize = self.checkbox(
            "Close button minimises to the tray",
            CONTENT_X + 16,
            250,
            468,
            24,
            C_MINIMIZE,
            4,
        );
        self.controls.dark_mode = self.checkbox(
            "Dark theme",
            CONTENT_X + 16,
            284,
            468,
            24,
            C_DARK_MODE,
            4,
        );
        self.hint(
            "The tray mode keeps the schedule and hotkeys running.",
            CONTENT_X + 16,
            316,
            468,
            22,
            4,
        );

        self.card("Hotkeys", CONTENT_X, 362, 500, 150, C_HOTKEY_GROUP, 4);
        self.controls.hotkeys = self.label(
            "Ctrl+Alt+N    toggle night light\nCtrl+Alt+↑ / ↓    brightness of the selected display",
            CONTENT_X + 16,
            402,
            468,
            90,
            4,
        );

        self.card("About", CONTENT_X + 516, 176, 288, 336, C_ABOUT_GROUP, 4);
        self.controls.about = self.label(
            "Luma 0.4.3\n\nNative Windows monitor control: brightness, every MCCS/VCP control, sunset-aware night light, and a time-based auto-dim curve.\n\nBrightness uses DDC/CI on external monitors and the panel API on laptops.\n\nGamma ramps are restored when Luma exits.",
            CONTENT_X + 532,
            216,
            256,
            276,
            4,
        );
        SendMessageW(self.controls.about, WM_SETFONT, self.fonts.small as usize, 1);

        self.controls.open_folder =
            self.button("Open settings folder", CONTENT_X + 516, 528, 140, 32, C_OPEN_FOLDER, false, 4);
        self.controls.reset =
            self.button("Reset settings…", CONTENT_X + 666, 528, 138, 32, C_RESET, false, 4);
        self.controls.settings_hint = self.hint(
            "Settings file:\n%APPDATA%\\Luma\\settings.ini",
            CONTENT_X + 516,
            572,
            288,
            56,
            4,
        );
    }

    unsafe fn create_footer(&mut self) {
        self.controls.footer = self.label("", CONTENT_X, 748, CONTENT_W, 22, 0);
        SendMessageW(self.controls.footer, WM_SETFONT, self.fonts.small as usize, 1);
        SetWindowLongPtrW(self.controls.footer, GWLP_USERDATA, 2);
    }


    unsafe fn on_drawitem(&mut self, d: &DRAWITEMSTRUCT) {
        let rc = d.rcItem;
        match d.CtlID as u16 {
            C_HEADER => self.paint_header(d.hDC, &rc),
            C_SIDEBAR => self.paint_sidebar(d.hDC, &rc),
            C_NAV0 => self.paint_nav(d.hDC, &rc, 0),
            C_NAV1 => self.paint_nav(d.hDC, &rc, 1),
            C_NAV2 => self.paint_nav(d.hDC, &rc, 3),
            C_NAV3 => self.paint_nav(d.hDC, &rc, 2),
            C_MON_GROUP | C_BRIGHT_GROUP | C_VCP_GROUP | C_NL_CUSTOM_GROUP | C_NL_SOLAR_GROUP
            | C_START_GROUP | C_HOTKEY_GROUP | C_ABOUT_GROUP | C_NL_TUNING_GROUP
            | C_DIM_GROUP_A | C_DIM_GROUP_B | C_DIM_GROUP_C => {
                let title = hwnd_text(d.hwndItem);
                self.paint_card(d.hDC, &rc, &title);
            }
            C_NL_PREVIEW => self.paint_tint_preview(d.hDC, &rc),
            C_DIM_CHART => self.paint_dim_chart(d.hDC, d.hwndItem),
            C_DIM_GAUGE => self.paint_dim_gauge(d.hDC, d.hwndItem),
            C_MON_LIST => self.paint_list_item(d.hDC, &rc, d.itemID as i32, d.itemState),
            C_VCP_COMBO | C_VCP_ENUM | C_NL_MODE | C_NL_LOCATION | C_NL_TZ => {
                // Dropdown rows are drawn for the list window, whose items only
                // the combo box itself can hand out.
                let combo = GetDlgItem(self.hwnd, d.CtlID as i32);
                let src = if combo.is_null() { d.hwndItem } else { combo };
                self.paint_combo_item(d.hDC, &rc, src, d.itemID as i32, d.itemState)
            }
            C_NL_ENABLE | C_NL_WEEKEND | C_DIM_ENABLE | C_AUTOSTART | C_MINIMIZE
            | C_DARK_MODE => {
                self.paint_checkbox(d.hDC, &rc, d.hwndItem, d.itemState, d.CtlID as u16)
            }
            C_NL_APPLY | C_VCP_SET | C_DIM_APPLY => {
                self.paint_primary(d.hDC, &rc, d.hwndItem, d.itemState)
            }
            C_MON_REFRESH | C_VCP_READ | C_VCP_USE | C_NL_RESET_TINT | C_NL_PAUSE
            | C_NL_USE_TZ | C_OPEN_FOLDER | C_RESET => {
                self.paint_secondary(d.hDC, &rc, d.hwndItem, d.itemState)
            }
            _ => {}
        }
    }

    unsafe fn paint_header(&self, hdc: HDC, rc: &RECT) {
        fill_color(hdc, rc, pal().header);
        let mut moon = RECT { left: 28, top: 26, right: 64, bottom: 62 };
        fill_ellipse(hdc, &moon, pal().accent);
        let mut cut = RECT { left: 46, top: 19, right: 82, bottom: 55 };
        fill_ellipse(hdc, &cut, pal().header);
        moon = RECT { left: 92, top: 20, right: 500, bottom: 58 };
        draw_text(hdc, "Luma", &mut moon, self.fonts.title, COL_WHITE, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        let mut sub = RECT { left: 94, top: 56, right: 720, bottom: 78 };
        draw_text(
            hdc,
            "Monitor control and night light",
            &mut sub,
            self.fonts.small,
            pal().header_sub,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        let mut ver = RECT { left: CLIENT_W - 120, top: 34, right: CLIENT_W - 28, bottom: 56 };
        draw_text(
            hdc,
            "v0.4.3",
            &mut ver,
            self.fonts.base,
            pal().header_sub,
            DT_RIGHT | DT_VCENTER | DT_SINGLELINE,
        );
    }

    unsafe fn paint_sidebar(&self, hdc: HDC, rc: &RECT) {
        fill_color(hdc, rc, pal().card);
        let edge = RECT { left: rc.right - 1, top: 0, right: rc.right, bottom: rc.bottom };
        fill_color(hdc, &edge, pal().border);
        let mut note = RECT {
            left: 18,
            top: rc.bottom - 52,
            right: rc.right - 16,
            bottom: rc.bottom - 20,
        };
        draw_text(
            hdc,
            "Brightness, DDC/CI and night light in one small native app.",
            &mut note,
            self.fonts.small,
            pal().muted,
            DT_LEFT | DT_WORDBREAK | DT_NOPREFIX,
        );
    }

    unsafe fn paint_nav(&self, hdc: HDC, rc: &RECT, index: usize) {
        let active = self.page == index;
        fill_color(hdc, rc, pal().card);
        let body = RECT { left: 0, top: 0, right: rc.right, bottom: rc.bottom };
        if active {
            round_fill(hdc, &body, 10, pal().accent_soft, pal().accent_soft);
        }
        let dot = RECT { left: 18, top: 19, right: 26, bottom: 27 };
        fill_ellipse(hdc, &dot, if active { pal().accent } else { pal().nav_dot });
        let labels = ["Displays", "Night light", "Auto dim", "Settings"];
        let mut text = RECT { left: 36, top: 0, right: rc.right - 8, bottom: rc.bottom };
        draw_text(
            hdc,
            labels[index],
            &mut text,
            self.fonts.bold,
            if active {
                self.accent_text()
            } else {
                pal().text
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
    }

    unsafe fn paint_card(&self, hdc: HDC, rc: &RECT, title: &str) {
        round_fill(hdc, rc, 12, pal().card, pal().border);
        let mut t = RECT { left: 18, top: 10, right: rc.right - 18, bottom: 34 };
        draw_text(
            hdc,
            title,
            &mut t,
            self.fonts.bold,
            pal().text,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
    }

    /// Accent ink that stays legible on the soft accent fill: the dark theme
    /// accent is light enough for text, the light theme one is not.
    fn accent_text(&self) -> u32 {
        if DARK_MODE.load(Ordering::Relaxed) {
            pal().accent
        } else {
            pal().accent_dark
        }
    }

    unsafe fn combo_item_text(hwnd: HWND, id: i32) -> String {
        if id < 0 {
            return String::new();
        }
        let mut buf = [0u16; 256];
        let n = SendMessageW(hwnd, CB_GETLBTEXT, id as usize, buf.as_mut_ptr() as isize);
        if n <= 0 {
            return String::new();
        }
        from_wide(&buf[..n as usize])
    }

    unsafe fn list_item_text(hwnd: HWND, id: i32) -> String {
        if id < 0 {
            return String::new();
        }
        let mut buf = [0u16; 256];
        let n = SendMessageW(hwnd, LB_GETTEXT, id as usize, buf.as_mut_ptr() as isize);
        if n <= 0 {
            return String::new();
        }
        from_wide(&buf[..n as usize])
    }

    unsafe fn paint_combo_item(
        &self,
        hdc: HDC,
        rc: &RECT,
        hwnd: HWND,
        item_id: i32,
        state: u32,
    ) {
        let face = state & ODS_COMBOBOXEDIT != 0;
        let selected = state & ODS_SELECTED != 0;
        if face {
            round_fill(hdc, rc, 6, pal().well, pal().border);
        } else {
            fill_color(hdc, rc, if selected { pal().accent_soft } else { pal().well });
        }
        let text = Self::combo_item_text(hwnd, item_id);
        let ink = if selected && !face {
            self.accent_text()
        } else {
            pal().text
        };
        let mut t = RECT {
            left: rc.left + 10,
            top: rc.top,
            right: if face { rc.right - 30 } else { rc.right - 6 },
            bottom: rc.bottom,
        };
        draw_text(
            hdc,
            &text,
            &mut t,
            self.fonts.base,
            ink,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    }

    unsafe fn paint_list_item(&self, hdc: HDC, rc: &RECT, item_id: i32, state: u32) {
        let selected = state & ODS_SELECTED != 0;
        fill_color(hdc, rc, if selected { pal().accent_soft } else { pal().well });
        let text = match self.mon_labels.get(item_id.max(0) as usize) {
            Some(t) => t.as_str(),
            None => "",
        };
        let mut t = RECT {
            left: rc.left + 10,
            top: rc.top,
            right: rc.right - 6,
            bottom: rc.bottom,
        };
        draw_text(
            hdc,
            text,
            &mut t,
            self.fonts.base,
            if selected { self.accent_text() } else { pal().text },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    }

    /// 24 hour preview of the auto-dim curve, with a marker for the current
    /// local time and hour ticks along the bottom.
    unsafe fn paint_dim_chart(&self, hdc_out: HDC, hwnd: HWND) {
        const SRCCOPY: u32 = 0x00CC_0020;
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        let cw = rc.right - rc.left;
        let ch = rc.bottom - rc.top;
        if cw <= 0 || ch <= 0 {
            return;
        }
        // Draw offscreen so a clipped WM_DRAWITEM DC cannot erase what is
        // already on screen; the blit only touches the damaged part.
        let mem = CreateCompatibleDC(hdc_out);
        let bmp = CreateCompatibleBitmap(hdc_out, cw, ch);
        let old = SelectObject(mem, bmp);
        let hdc = mem;
        fill_color(hdc, &rc, pal().card);
        let body = RECT {
            left: rc.left,
            top: rc.top,
            right: rc.right,
            bottom: rc.bottom,
        };
        round_fill(hdc, &body, 10, pal().well, pal().border);
        let plot = RECT {
            left: rc.left + 10,
            top: rc.top + 12,
            right: rc.right - 10,
            bottom: rc.bottom - 22,
        };
        let w = (plot.right - plot.left).max(1);
        let h = (plot.bottom - plot.top).max(1);
        let curve = self.dim_curve();
        if !dim::valid(&curve) {
            let mut msg = RECT {
                left: rc.left + 10,
                top: plot.top,
                right: rc.right - 10,
                bottom: plot.bottom,
            };
            draw_text(
                hdc,
                "Fix the times below to see the curve.",
                &mut msg,
                self.fonts.small,
                pal().muted,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            BitBlt(hdc_out, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
            return;
        }
        for q in 1..4 {
            let y = plot.top + h * q / 4;
            let line = RECT {
                left: plot.left,
                top: y,
                right: plot.right,
                bottom: y + 1,
            };
            fill_color(hdc, &line, pal().border);
        }
        let mut prev_y = -1;
        for i in 0..w {
            let minute = (i as i64 * 1439 / w as i64) as u16;
            let pct = dim::target(&curve, minute) as i32;
            let y = plot.bottom - (h * pct / 100).clamp(1, h);
            let bar = RECT {
                left: plot.left + i,
                top: y,
                right: plot.left + i + 1,
                bottom: plot.bottom,
            };
            fill_color(hdc, &bar, pal().accent_soft);
            // Join each cap to the previous one so steep fades stay a line
            // instead of breaking into dashes.
            let (top, bottom) = if prev_y < 0 {
                (y, y + 2)
            } else {
                (prev_y.min(y), prev_y.max(y) + 2)
            };
            let cap = RECT {
                left: plot.left + i,
                top,
                right: plot.left + i + 1,
                bottom,
            };
            fill_color(hdc, &cap, pal().accent);
            prev_y = y;
        }
        let now = local_minute();
        let x = plot.left + (w as i64 * now as i64 / 1439) as i32;
        let marker = RECT {
            left: x,
            top: plot.top,
            right: x + 2,
            bottom: plot.bottom,
        };
        fill_color(hdc, &marker, pal().text);
        for hour in 0..5 {
            let hx = plot.left + w * hour / 4;
            let tick = RECT {
                left: hx,
                top: plot.bottom,
                right: hx + 1,
                bottom: plot.bottom + 4,
            };
            fill_color(hdc, &tick, pal().border);
            let mut label = RECT {
                left: hx - 20,
                top: plot.bottom + 4,
                right: hx + 20,
                bottom: plot.bottom + 20,
            };
            draw_text(
                hdc,
                &format!("{:02}:00", hour * 6),
                &mut label,
                self.fonts.small,
                pal().muted,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
        BitBlt(hdc_out, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
    }

    /// Horizontal fill showing the current brightness target.
    unsafe fn paint_dim_gauge(&self, hdc_out: HDC, hwnd: HWND) {
        const SRCCOPY: u32 = 0x00CC_0020;
        let mut rc: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut rc);
        let cw = rc.right - rc.left;
        let ch = rc.bottom - rc.top;
        if cw <= 0 || ch <= 0 {
            return;
        }
        let mem = CreateCompatibleDC(hdc_out);
        let bmp = CreateCompatibleBitmap(hdc_out, cw, ch);
        let old = SelectObject(mem, bmp);
        let hdc = mem;
        fill_color(hdc, &rc, pal().card);
        round_fill(hdc, &rc, 5, pal().well, pal().border);
        let curve = self.dim_curve();
        let pct = if dim::valid(&curve) {
            dim::target(&curve, local_minute()) as i32
        } else {
            0
        };
        let w = cw * pct / 100;
        if w > 0 {
            let fill = RECT {
                left: rc.left,
                top: rc.top,
                right: rc.left + w,
                bottom: rc.bottom,
            };
            round_fill(hdc, &fill, 5, pal().accent, pal().accent);
        }
        BitBlt(hdc_out, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old);
        DeleteObject(bmp);
        DeleteDC(mem);
    }

    unsafe fn paint_tint_preview(&self, hdc: HDC, rc: &RECT) {
        let ramp = crate::night::build_ramp(self.settings.temp, self.settings.strength as f32 / 100.0);
        let red = (ramp[255] >> 8) as u8;
        let green = (ramp[511] >> 8) as u8;
        let blue = (ramp[767] >> 8) as u8;
        let tint = rgb(red as u32, green as u32, blue as u32);
        let top = rc.top + 2;
        let bottom = rc.bottom - 2;
        let mid = (rc.left + rc.right) / 2;
        let left = RECT {
            left: rc.left + 4,
            top,
            right: mid - 4,
            bottom,
        };
        let right = RECT {
            left: mid + 4,
            top,
            right: rc.right - 4,
            bottom,
        };
        round_fill(hdc, &left, 8, COL_WHITE, pal().border);
        round_fill(hdc, &right, 8, tint, pal().border);
        let mut ltext = left;
        draw_text(
            hdc,
            "Before",
            &mut ltext,
            self.fonts.small,
            rgb(0x2C, 0x31, 0x3A),
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        let luminance =
            (299 * red as u32 + 587 * green as u32 + 114 * blue as u32) / 1000;
        // The chip is a fixed preview colour, so its ink must be fixed too:
        // pal().text flips to near-white in dark mode, which disappears on a
        // pale tint. Always pair a pale chip with dark ink.
        let ink = if luminance > 140 {
            rgb(0x2C, 0x31, 0x3A)
        } else {
            COL_WHITE
        };
        let mut rtext = right;
        let label = format!("{} K · {}%", self.settings.temp, self.settings.strength);
        draw_text(
            hdc,
            &label,
            &mut rtext,
            self.fonts.small,
            ink,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
    }

    unsafe fn paint_primary(&self, hdc: HDC, rc: &RECT, hwnd: HWND, state: u32) {
        let disabled = state & ODS_DISABLED != 0;
        let pressed = state & ODS_SELECTED != 0;
        let fill = if disabled {
            pal().disabled_bg
        } else if pressed {
            pal().accent_dark
        } else {
            pal().accent
        };
        round_fill(hdc, rc, 8, fill, fill);
        let text = hwnd_text(hwnd);
        let mut t = RECT { left: 4, top: 0, right: rc.right - 4, bottom: rc.bottom };
        draw_text(
            hdc,
            &text,
            &mut t,
            self.fonts.bold,
            if disabled { pal().disabled_text } else { pal().accent_ink },
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        if state & ODS_FOCUS != 0 && !disabled {
            let f = RECT { left: 3, top: 3, right: rc.right - 3, bottom: rc.bottom - 3 };
            let inner = RECT { left: 4, top: 4, right: rc.right - 4, bottom: rc.bottom - 4 };
            round_fill(hdc, &f, 7, pal().accent_dark, pal().accent_dark);
            round_fill(hdc, &inner, 6, fill, fill);
            let mut t2 = RECT { left: 4, top: 0, right: rc.right - 4, bottom: rc.bottom };
            draw_text(
                hdc,
                &text,
                &mut t2,
                self.fonts.bold,
                if disabled { pal().disabled_text } else { pal().accent_ink },
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }
    unsafe fn paint_secondary(&self, hdc: HDC, rc: &RECT, hwnd: HWND, state: u32) {
        let disabled = state & ODS_DISABLED != 0;
        let pressed = state & ODS_SELECTED != 0;
        let fill = if pressed {
            pal().accent_soft
        } else {
            pal().well
        };
        round_fill(hdc, rc, 8, fill, pal().border);
        let text = hwnd_text(hwnd);
        let mut t = RECT { left: 4, top: 0, right: rc.right - 4, bottom: rc.bottom };
        draw_text(
            hdc,
            &text,
            &mut t,
            self.fonts.base,
            if disabled { pal().disabled_text } else { pal().text },
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        if state & ODS_FOCUS != 0 && !disabled {
            let f = RECT { left: 2, top: 2, right: rc.right - 2, bottom: rc.bottom - 2 };
            let frame = CreatePen(0, 1, pal().accent);
            let old = SelectObject(hdc, frame as HANDLE);
            let ob = SelectObject(hdc, GetStockObject(NULL_BRUSH) as HANDLE);
            Rectangle(hdc, f.left, f.top, f.right, f.bottom);
            SelectObject(hdc, ob);
            SelectObject(hdc, old);
            DeleteObject(frame as HANDLE);
        }
    }

    unsafe fn paint_checkbox(&self, hdc: HDC, rc: &RECT, hwnd: HWND, state: u32, id: u16) {
        let checked = match id {
            C_NL_ENABLE => self.settings.enabled,
            C_NL_WEEKEND => self.settings.weekend,
            C_DIM_ENABLE => self.settings.auto_dim,
            C_AUTOSTART => self.settings.autostart,
            C_MINIMIZE => self.settings.minimize_tray,
            C_DARK_MODE => self.settings.dark,
            _ => false,
        };
        let disabled = state & ODS_DISABLED != 0;
        let on_card = matches!(
            id,
            C_AUTOSTART | C_MINIMIZE | C_DARK_MODE | C_NL_WEEKEND
        );
        fill_color(hdc, rc, if on_card { pal().card } else { pal().bg });
        let size = 16;
        let top = rc.top + (rc.bottom - rc.top - size) / 2;
        let boxr = RECT { left: rc.left + 2, top, right: rc.left + 2 + size, bottom: top + size };
        round_fill(
            hdc,
            &boxr,
            3,
            if checked { pal().accent } else { pal().card },
            if checked { pal().accent } else { pal().muted },
        );
        if checked {
            let mut g = RECT { left: boxr.left, top: boxr.top - 2, right: boxr.right, bottom: boxr.bottom };
            draw_text(
                hdc,
                "\u{2713}",
                &mut g,
                self.fonts.small,
                pal().accent_ink,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
        let text = hwnd_text(hwnd);
        let mut t = RECT { left: rc.left + 26, top: rc.top, right: rc.right - 2, bottom: rc.bottom };
        let bold = matches!(id, C_DIM_ENABLE | C_NL_ENABLE);
        draw_text(
            hdc,
            &text,
            &mut t,
            if bold { self.fonts.bold } else { self.fonts.base },
            if disabled { pal().disabled_text } else { pal().text },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE,
        );
        if state & ODS_FOCUS != 0 && !disabled {
            let f = RECT {
                left: boxr.left - 2,
                top: boxr.top - 2,
                right: boxr.right + 2,
                bottom: boxr.bottom + 2,
            };
            let frame = CreatePen(PS_DOT, 1, pal().accent);
            let old = SelectObject(hdc, frame as HANDLE);
            let ob = SelectObject(hdc, GetStockObject(NULL_BRUSH) as HANDLE);
            Rectangle(hdc, f.left, f.top, f.right, f.bottom);
            SelectObject(hdc, ob);
            SelectObject(hdc, old);
            DeleteObject(frame as HANDLE);
        }
    }

    // ---------- pages ----------

    unsafe fn set_page(&mut self, page: usize) {
        self.page = page.min(3);
        for i in 0..self.pages.len() {
            let show = i == self.page;
            for &h in &self.pages[i] {
                ShowWindow(h, if show { SW_SHOW } else { SW_HIDE });
                if show {
                    InvalidateRect(h, std::ptr::null(), 1);
                }
            }
        }
        for id in [C_NAV0, C_NAV1, C_NAV2, C_NAV3] {
            let h = GetDlgItem(self.hwnd, id as i32);
            InvalidateRect(h, std::ptr::null(), 1);
        }
        self.sync_page_visibility();
    }

    unsafe fn sync_page_visibility(&mut self) {
        if self.page != 0 {
            return;
        }
        let panel_vis = self.panel_brightness.is_some();
        for &h in &[self.controls.panel_caption, self.controls.panel, self.controls.panel_label] {
            ShowWindow(h, SW_SHOW);
        }
        EnableWindow(self.controls.panel, panel_vis as i32);
        if !panel_vis {
            set_text(self.controls.panel_label, "not available");
        }
        ShowWindow(
            self.controls.vcp_enum,
            if self.vcp_enum_visible { SW_SHOW } else { SW_HIDE },
        );
    }

    // ---------- settings UI ----------

    unsafe fn populate_settings_ui(&mut self) {
        let s = &self.settings;
        SendMessageW(
            self.controls.nl_enable,
            BM_SETCHECK,
            if s.enabled { BST_CHECKED } else { 0 },
            0,
        );
        set_combo_selection(
            self.controls.nl_mode,
            match s.mode.as_str() {
                "custom" => 1,
                "always" => 2,
                _ => 0,
            },
        );
        set_text(self.controls.lat, &format!("{:.4}", s.lat));
        set_text(self.controls.lon, &format!("{:.4}", s.lon));
        set_combo_selection(self.controls.tz, tz_selection(s.tz));
        set_combo_selection(
            self.controls.nl_location,
            city_selection(s.lat, s.lon).unwrap_or(CITIES.len()),
        );
        set_text(self.controls.wd_start, &s.wd_start);
        set_text(self.controls.wd_end, &s.wd_end);
        set_text(self.controls.we_start, &s.we_start);
        set_text(self.controls.we_end, &s.we_end);
        SendMessageW(
            self.controls.weekend,
            BM_SETCHECK,
            if s.weekend { BST_CHECKED } else { 0 },
            0,
        );
        set_slider_range(self.controls.temp, 1000, 6500);
        SendMessageW(self.controls.temp, TBM_SETTICFREQ, 500, 0);
        set_slider(self.controls.temp, s.temp as i32);
        set_slider_range(self.controls.strength, 0, 100);
        SendMessageW(self.controls.strength, TBM_SETTICFREQ, 10, 0);
        set_slider(self.controls.strength, s.strength as i32);
        set_slider_range(self.controls.fade, 0, 240);
        SendMessageW(self.controls.fade, TBM_SETTICFREQ, 30, 0);
        set_slider(self.controls.fade, s.fade as i32);
        let (autostart, minimize_tray) = (s.autostart, s.minimize_tray);
        self.update_night_control_labels();
        SendMessageW(
            self.controls.autostart,
            BM_SETCHECK,
            if autostart { BST_CHECKED } else { 0 },
            0,
        );
        SendMessageW(
            self.controls.minimize,
            BM_SETCHECK,
            if minimize_tray { BST_CHECKED } else { 0 },
            0,
        );
        SendMessageW(
            self.controls.dark_mode,
            BM_SETCHECK,
            if self.settings.dark { BST_CHECKED } else { 0 },
            0,
        );
        set_text(self.controls.vcp_custom, "");
        SendMessageW(
            self.controls.dim_enable,
            BM_SETCHECK,
            if self.settings.auto_dim { BST_CHECKED } else { 0 },
            0,
        );
        set_text(self.controls.dim_evening, &self.settings.dim_evening);
        set_text(self.controls.dim_night, &self.settings.dim_night);
        set_text(self.controls.dim_day, &self.settings.dim_day);
        set_text(
            self.controls.dim_day_pct,
            &self.settings.dim_day_pct.to_string(),
        );
        set_text(
            self.controls.dim_evening_pct,
            &self.settings.dim_evening_pct.to_string(),
        );
        set_text(
            self.controls.dim_night_pct,
            &self.settings.dim_night_pct.to_string(),
        );
        set_text(self.controls.dim_fade, &self.settings.dim_fade.to_string());
        self.refresh_dim_labels();
    }

    fn dim_curve(&self) -> dim::Curve {
        dim::Curve {
            day: time_minutes(&self.settings.dim_day, 420) as u16,
            evening: time_minutes(&self.settings.dim_evening, 1260) as u16,
            night: time_minutes(&self.settings.dim_night, 1380) as u16,
            day_pct: self.settings.dim_day_pct.min(100) as u8,
            evening_pct: self.settings.dim_evening_pct.min(100) as u8,
            night_pct: self.settings.dim_night_pct.min(100) as u8,
            fade: self.settings.dim_fade.min(240) as u16,
        }
    }

    unsafe fn refresh_dim_labels(&mut self) {
        let curve = self.dim_curve();
        let minute = local_minute();
        if !dim::valid(&curve) {
            if self.dim_last_status != "invalid" {
                set_text(self.controls.dim_target, "—");
                set_text(
                    self.controls.dim_phase,
                    "Check the times: day must be first, then evening, then night.",
                );
                self.dim_last_status = "invalid".into();
            }
            return;
        }
        let (pct, phase) = dim::target_phase(&curve, minute);
        let (mins, next) = dim::next_change(&curve, minute);
        let label = match phase {
            dim::PHASE_MORNING => "Morning fade",
            dim::PHASE_DAY => "Daytime",
            dim::PHASE_EVENING => "Evening",
            _ => "Night",
        };
        let status = format!(
            "{}|{}|{}|{}",
            pct,
            label,
            mins,
            self.settings.auto_dim
        );
        if self.dim_last_status != status {
            set_text(self.controls.dim_target, &format!("{}%", pct));
            set_text(
                self.controls.dim_phase,
                &format!(
                    "{} now · next {} in {}h {:02}m{}",
                    label,
                    next,
                    mins / 60,
                    mins % 60,
                    if self.settings.auto_dim { "" } else { " · disabled" }
                ),
            );
            self.dim_last_status = status;
        }
    }

    unsafe fn apply_dim_edits(&mut self) {
        let evening = normalize_time(&hwnd_text(self.controls.dim_evening));
        let night = normalize_time(&hwnd_text(self.controls.dim_night));
        let day = normalize_time(&hwnd_text(self.controls.dim_day));
        let (Some(evening), Some(night), Some(day)) = (evening, night, day) else {
            return self.schedule_error("Auto-dim times must be 24-hour HH:MM.");
        };
        let day_pct: u32 = match hwnd_text(self.controls.dim_day_pct).trim().parse() {
            Ok(v) if (1..=100).contains(&v) => v,
            _ => return self.schedule_error("Day brightness must be between 1 and 100."),
        };
        let evening_pct: u32 = match hwnd_text(self.controls.dim_evening_pct).trim().parse() {
            Ok(v) if (1..=100).contains(&v) => v,
            _ => return self.schedule_error("Evening brightness must be between 1 and 100."),
        };
        let night_pct: u32 = match hwnd_text(self.controls.dim_night_pct).trim().parse() {
            Ok(v) if (1..=100).contains(&v) => v,
            _ => return self.schedule_error("Night brightness must be between 1 and 100."),
        };
        let fade: u32 = match hwnd_text(self.controls.dim_fade).trim().parse() {
            Ok(v) if (1..=240).contains(&v) => v,
            _ => return self.schedule_error("Fade must be between 1 and 240 minutes."),
        };
        let d = time_minutes(&day, 420);
        let e = time_minutes(&evening, 1260);
        let n = time_minutes(&night, 1380);
        if !(d < e && e < n) {
            return self.schedule_error(
                "Auto-dim anchors must be ordered: back-to-day, then evening, then night.",
            );
        }
        if day_pct < evening_pct {
            return self.schedule_error("Day brightness should be at least evening brightness.");
        }
        if evening_pct < night_pct {
            return self.schedule_error("Evening brightness should be at least night brightness.");
        }
        self.settings.dim_evening = evening;
        self.settings.dim_night = night;
        self.settings.dim_day = day;
        self.settings.dim_day_pct = day_pct;
        self.settings.dim_evening_pct = evening_pct;
        self.settings.dim_night_pct = night_pct;
        self.settings.dim_fade = fade;
        self.mark_dirty();
        self.refresh_dim_labels();
        self.set_footer("Auto-dim schedule saved.");
        self.dim_last_pct = None;
    }

    unsafe fn apply_auto_dim(&mut self, pct: u8, force: bool) {
        let now = GetTickCount64();
        if !force && now.saturating_sub(self.last_manual_bright) < 300_000 {
            return;
        }
        if !force && self.dim_last_pct == Some(pct) {
            return;
        }
        let mut applied = 0;
        for d in self.displays.iter_mut() {
            if let Some(b) = d.brightness {
                let raw = (pct as u32).saturating_mul(b.max) / 100;
                if d.set_brightness(raw) {
                    applied += 1;
                }
            }
        }
        if applied > 0 {
            self.dim_last_pct = Some(pct);
            self.dim_last_apply = now;
            if let Some(d) = self.displays.get(self.selected) {
                if let Some(b) = d.brightness {
                    let shown = (pct as u32).saturating_mul(b.max) / 100;
                    let raw_pct = if b.max > 0 { shown * 100 / b.max } else { 0 };
                    set_slider(self.controls.bright, raw_pct as i32);
                    set_text(self.controls.bright_label, &format!("{}%", raw_pct));
                }
            }
            self.refresh_footer();
        } else {
            set_text(
                self.controls.dim_status,
                "Auto dim could not write brightness: no display accepted a DDC/CI value. \
                 Enable DDC/CI in the monitor's OSD.",
            );
        }
    }

    unsafe fn update_night_control_labels(&mut self) {
        set_text(
            self.controls.temp_label,
            &format!("{} K", self.settings.temp),
        );
        set_text(
            self.controls.strength_label,
            &format!("{}%", self.settings.strength),
        );
        set_text(
            self.controls.fade_label,
            &format!("{} min", self.settings.fade),
        );
        self.refresh_tint_preview();
    }

    unsafe fn refresh_tint_preview(&self) {
        if !self.controls.tint_preview.is_null() {
            InvalidateRect(self.controls.tint_preview, std::ptr::null(), 0);
        }
    }

    /// Store a solar location and mirror it into the Night light controls.
    unsafe fn set_solar_location(&mut self, lat: f64, lon: f64, tz: f64) {
        self.settings.lat = lat;
        self.settings.lon = lon;
        self.settings.tz = tz;
        set_text(self.controls.lat, &format!("{:.4}", lat));
        set_text(self.controls.lon, &format!("{:.4}", lon));
        set_combo_selection(self.controls.tz, tz_selection(tz));
        set_combo_selection(
            self.controls.nl_location,
            city_selection(lat, lon).unwrap_or(CITIES.len()),
        );
        self.mark_dirty();
        self.refresh_night_now();
    }

    unsafe fn refresh_night_now(&mut self) {
        let now = night::now_info();
        let info = night::compute(&self.settings, now, now_epoch());
        self.apply_night(&info);
        self.update_night_labels(&info, now);
    }

    unsafe fn update_night_labels(&mut self, info: &NightInfo, now: night::LocalNow) {
        if info.status != self.last_nl_status {
            set_text(self.controls.nl_status, &info.status);
            self.last_nl_status = info.status.clone();
        }
        set_text(self.controls.sun_info, &solar_summary(&self.settings, now));
        let paused = self.settings.paused_until > now_epoch();
        set_text(
            self.controls.pause,
            if paused { "Resume now" } else { "Pause 1 hour" },
        );
    }

    fn apply_night(&mut self, info: &NightInfo) {
        unsafe {
            if info.blend <= 0.001 {
                if self.applied_any {
                    self.restore_original_gamma();
                }
                self.last_blend = 0.0;
                return;
            }
            let now = GetTickCount64();
            let changed = (info.blend - self.last_blend).abs() > 0.002;
            if !changed && now.saturating_sub(self.last_apply) < 5000 {
                return;
            }
            let ramp = night::build_ramp(info.kelvin, info.blend);
            let mut applied = 0usize;
            let mut devices: Vec<String> = Vec::new();
            for d in &self.displays {
                if !devices.iter().any(|x| x == &d.device) {
                    devices.push(d.device.clone());
                }
            }
            for dev in &devices {
                if !self.original_gamma.iter().any(|(d, _)| d == dev) {
                    let original = read_gamma(dev).unwrap_or_else(night::identity_ramp);
                    self.original_gamma.push((dev.clone(), original));
                }
                if apply_gamma(dev, &ramp) {
                    applied += 1;
                }
            }
            if applied > 0 {
                self.applied_any = true;
                self.last_blend = info.blend;
                self.last_apply = now;
            }
            if applied < devices.len() {
                self.set_footer(&format!(
                    "Gamma ramp unavailable on {}/{} display(s). HDR or a locked display mode can block tinting.",
                    devices.len() - applied,
                    devices.len()
                ));
            }
        }
    }

    unsafe fn restore_original_gamma(&mut self) {
        for (dev, ramp) in &self.original_gamma {
            apply_gamma(dev, ramp);
        }
        self.applied_any = false;
        self.last_blend = 0.0;
    }

    // ---------- monitors ----------

    unsafe fn refresh_monitors(&mut self) {
        let old_device = self.displays.get(self.selected).map(|d| d.device.clone());
        self.displays = monitors::enumerate();
        if self.displays.is_empty() {
            self.selected = 0;
        } else if let Some(old) = old_device {
            self.selected = self
                .displays
                .iter()
                .position(|d| d.device == old)
                .unwrap_or(0);
        } else {
            self.selected = self.selected.min(self.displays.len() - 1);
        }
        self.settings.selected = self.selected;

        SendMessageW(self.controls.list, LB_RESETCONTENT, 0, 0);
        self.mon_labels.clear();
        for d in &self.displays {
            let w = wide(&d.label());
            self.mon_labels.push(d.label());
            SendMessageW(self.controls.list, LB_ADDSTRING, 0, w.as_ptr() as isize);
        }
        if !self.displays.is_empty() {
            SendMessageW(self.controls.list, LB_SETCURSEL, self.selected, 0);
        }
        self.refresh_display_ui();
        self.refresh_footer();
    }

    unsafe fn refresh_display_ui(&mut self) {
        if self.displays.is_empty() {
            set_text(self.controls.disp_status, "No displays were detected.");
            EnableWindow(self.controls.bright, 0);
            EnableWindow(self.controls.vcp_slider, 0);
            EnableWindow(self.controls.vcp_edit, 0);
            EnableWindow(self.controls.vcp_set, 0);
            set_text(self.controls.bright_label, "—");
            return;
        }
        let idx = self.selected.min(self.displays.len() - 1);
        self.selected = idx;
        if !self.displays[idx].has_ddc() || self.displays[idx].brightness.is_none() {
            self.displays[idx].reconnect();
        }
        let brightness = self.displays[idx].read_brightness();
        let ddc = self.displays[idx].has_ddc();

        match brightness {
            Some(b) => {
                let pct = if b.max > 0 {
                    (b.current.saturating_mul(100) / b.max).min(100)
                } else {
                    0
                };
                set_slider_range(self.controls.bright, 0, 100);
                set_slider(self.controls.bright, pct as i32);
                set_text(self.controls.bright_label, &format!("{}%", pct));
                EnableWindow(self.controls.bright, 1);
            }
            None => {
                set_text(self.controls.bright_label, "not supported");
                EnableWindow(self.controls.bright, 0);
            }
        }

        if ddc {
            EnableWindow(self.controls.vcp_combo, 1);
            EnableWindow(self.controls.vcp_read, 1);
            EnableWindow(self.controls.vcp_slider, 1);
            EnableWindow(self.controls.vcp_edit, 1);
            EnableWindow(self.controls.vcp_set, 1);
            self.read_vcp();
            let b = self.displays[idx].brightness;
            let detail = match b {
                Some(b) if b.dxva => format!("DDC/CI · brightness via DXVA2 (0–{})", b.max),
                Some(b) => format!("DDC/CI · brightness via VCP 0x10 (0–{})", b.max),
                None => "DDC/CI · brightness unavailable".to_string(),
            };
            set_text(
                self.controls.disp_status,
                &format!("{}  ·  {}", self.displays[idx].device, detail),
            );
        } else {
            EnableWindow(self.controls.vcp_combo, 0);
            EnableWindow(self.controls.vcp_read, 0);
            EnableWindow(self.controls.vcp_slider, 0);
            EnableWindow(self.controls.vcp_edit, 0);
            EnableWindow(self.controls.vcp_set, 0);
            EnableWindow(self.controls.vcp_custom, 0);
            EnableWindow(self.controls.vcp_use, 0);
            set_text(self.controls.vcp_edit, "");
            set_text(self.controls.vcp_value, "");
            set_text(
                self.controls.vcp_max,
                "No DDC/CI controls on this display",
            );
            set_text(
                self.controls.disp_status,
                "This display does not expose DDC/CI. If it is a laptop panel, use the Panel slider (WMI). \
                 Otherwise check the monitor's OSD: DDC/CI must be enabled, and some hubs/docks block it.",
            );
        }
        self.sync_page_visibility();
    }

    unsafe fn read_vcp(&mut self) {
        if self.displays.is_empty() {
            return;
        }
        let idx = self.selected.min(self.displays.len() - 1);
        let code = self.vcp_code;
        let got = self.displays[idx].get_vcp(code);
        let name = vcp_name(code);
        match got {
            Some((cur, max)) => {
                self.vcp_max = max.max(1);
                set_slider_range(self.controls.vcp_slider, 0, self.vcp_max as i32);
                set_slider(self.controls.vcp_slider, cur.min(self.vcp_max) as i32);
                set_text(
                    self.controls.vcp_value,
                    &format!("{} / {}", cur, max),
                );
                set_text(
                    self.controls.vcp_max,
                    &format!("{} (0x{:02X}) · range 0–{}", name, code, max),
                );
                set_text(self.controls.vcp_edit, &cur.to_string());
                EnableWindow(self.controls.vcp_slider, 1);
                EnableWindow(self.controls.vcp_set, 1);
                self.fill_vcp_enum(code, Some(cur));
            }
            None => {
                self.vcp_max = 0;
                set_slider_range(self.controls.vcp_slider, 0, 1);
                set_slider(self.controls.vcp_slider, 0);
                set_text(self.controls.vcp_value, "—");
                set_text(
                    self.controls.vcp_max,
                    &format!("{} (0x{:02X}) · not readable on this display", name, code),
                );
                set_text(self.controls.vcp_edit, "");
                EnableWindow(self.controls.vcp_slider, 0);
                EnableWindow(self.controls.vcp_set, 1);
                self.fill_vcp_enum(code, None);
            }
        }
    }

    unsafe fn fill_vcp_enum(&mut self, code: u8, current: Option<u32>) {
        SendMessageW(self.controls.vcp_enum, CB_RESETCONTENT, 0, 0);
        let values = enum_values(code);
        if values.is_empty() {
            self.vcp_enum_visible = false;
        } else {
            self.vcp_enum_visible = true;
            let mut sel = None;
            for (i, (v, label)) in values.iter().enumerate() {
                add_combo_string(self.controls.vcp_enum, &format!("{} — {}", v, label));
                if Some(*v) == current {
                    sel = Some(i);
                }
            }
            if let Some(i) = sel {
                set_combo_selection(self.controls.vcp_enum, i);
            }
        }
        self.sync_page_visibility();
    }

    unsafe fn set_brightness_from_slider(&mut self) {
        self.last_manual_bright = GetTickCount64();
        if self.displays.is_empty() {
            return;
        }
        let pos = SendMessageW(self.controls.bright, TBM_GETPOS, 0, 0) as u32;
        let idx = self.selected.min(self.displays.len() - 1);
        let max = self.displays[idx].brightness.map(|b| b.max).unwrap_or(100).max(1);
        let raw = pos.saturating_mul(max) / 100;
        if self.displays[idx].set_brightness(raw) {
            self.refresh_footer();
        } else {
            set_text(
                self.controls.disp_status,
                "Brightness write failed. DDC/CI may be disabled on the monitor.",
            );
        }
    }

    unsafe fn set_panel_from_slider(&mut self) {
        let pos = SendMessageW(self.controls.panel, TBM_GETPOS, 0, 0) as u32;
        if monitors::wmi_set_panel_brightness(pos) {
            self.panel_brightness = Some(pos);
            set_text(self.controls.panel_label, &format!("{}%", pos));
            self.refresh_footer();
        } else {
            self.set_footer("Could not set the internal panel brightness (WMI unavailable).");
        }
    }

    unsafe fn vcp_write(&mut self, value: u32) {
        if self.displays.is_empty() {
            return;
        }
        let idx = self.selected.min(self.displays.len() - 1);
        let code = self.vcp_code;
        let mut value = value;
        if self.vcp_max > 0 && value > self.vcp_max {
            value = self.vcp_max;
        }
        if self.displays[idx].set_vcp(code, value) {
            let mut seen = None;
            for _ in 0..5 {
                sleep_ms(80);
                if let Some((cur, _)) = self.displays[idx].get_vcp(code) {
                    seen = Some(cur);
                    if cur == value {
                        break;
                    }
                }
            }
            self.read_vcp();
            if code == 0x10 && self.vcp_max > 0 {
                let pct = (value as f64 * 100.0 / self.vcp_max as f64).round() as i32;
                set_slider(self.controls.bright, pct);
                set_text(self.controls.bright_label, &format!("{}%", pct));
            }
            match seen {
                Some(cur) if cur == value => self.set_footer(&format!(
                    "VCP 0x{code:02X} set to {value} (read back {cur})."
                )),
                Some(cur) => self.set_footer(&format!(
                    "Sent {value} to VCP 0x{code:02X}, but the display reports {cur}. It may ignore this control."
                )),
                None => self.set_footer(&format!(
                    "Sent {value} to VCP 0x{code:02X}, but reading it back failed."
                )),
            }
        } else {
            set_text(
                self.controls.disp_status,
                &format!("Write of VCP 0x{code:02X} failed."),
            );
            self.set_footer(&format!(
                "Write of VCP 0x{code:02X} = {value} failed. DDC/CI may be disabled on the monitor."
            ));
        }
    }

    unsafe fn select_monitor(&mut self, index: usize) {
        if index < self.displays.len() {
            self.selected = index;
            self.settings.selected = index;
            self.mark_dirty();
            self.refresh_display_ui();
        }
    }

    // ---------- status ----------

    unsafe fn set_footer(&mut self, text: &str) {
        if text != self.last_footer {
            set_text(self.controls.footer, text);
            self.last_footer = text.to_string();
        }
    }

    unsafe fn refresh_footer(&mut self) {
        if self.displays.is_empty() {
            self.set_footer("No displays detected.");
            return;
        }
        let idx = self.selected.min(self.displays.len() - 1);
        let d = &self.displays[idx];
        let mut parts = vec![format!(
            "{} display{}",
            self.displays.len(),
            if self.displays.len() == 1 { "" } else { "s" }
        )];
        parts.push(d.name.clone());
        if d.has_ddc() {
            parts.push("DDC/CI".into());
        } else {
            parts.push("no DDC/CI".into());
        }
        if let Some(p) = self.panel_brightness {
            parts.push(format!("panel {}%", p));
        }
        if self.settings.enabled {
            parts.push(if self.settings.paused_until > now_epoch() {
                "night light paused".into()
            } else {
                "night light on".into()
            });
        }
        self.set_footer(&parts.join("  ·  "));
    }

    unsafe fn mark_dirty(&mut self) {
        self.dirty = true;
        self.dirty_since = GetTickCount64();
    }

    // ---------- tick ----------

    unsafe fn tick(&mut self) {
        let now = GetTickCount64();
        if !self.panel_checked && now.saturating_sub(self.started) > 700 {
            self.panel_checked = true;
            if let Some(v) = monitors::wmi_panel_brightness() {
                self.panel_brightness = Some(v);
                set_slider(self.controls.panel, v as i32);
                set_text(self.controls.panel_label, &format!("{}%", v));
                self.sync_page_visibility();
                self.refresh_footer();
            } else {
                self.sync_page_visibility();
            }
        }

        let local = night::now_info();
        let info = night::compute(&self.settings, local, now_epoch());
        self.apply_night(&info);
        self.update_night_labels(&info, local);
        self.refresh_dim_labels();
        if self.page == 2 {
            // The curve marker and the gauge follow the local clock.
            InvalidateRect(self.controls.dim_chart, std::ptr::null(), 0);
            InvalidateRect(self.controls.dim_gauge, std::ptr::null(), 0);
        }
        if self.settings.auto_dim {
            let curve = self.dim_curve();
            if dim::valid(&curve) {
                let (pct, _) = dim::target_phase(&curve, local_minute());
                if self.dim_last_pct != Some(pct)
                    && now.saturating_sub(self.dim_last_apply) > 10_000
                {
                    self.apply_auto_dim(pct, false);
                }
            }
        }

        if self.dirty && now.saturating_sub(self.dirty_since) > 1200 {
            self.settings.save();
            self.dirty = false;
        }
    }

    // ---------- tray ----------

    unsafe fn add_tray(&mut self) {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = self.icon;
        let tip = wide("Luma — monitor controls & night light");
        let n = tip.len().min(127);
        nid.szTip[..n].copy_from_slice(&tip[..n]);
        let ok = Shell_NotifyIconW(NIM_ADD, &nid);
        let err = GetLastError();
        self.log_tray(&format!(
            "NIM_ADD ok={ok} err={err} cbSize={} hwnd={:p} icon={:p} visible={}",
            nid.cbSize,
            self.hwnd,
            self.icon,
            IsWindowVisible(self.hwnd) as i32
        ));
        if ok != 0 {
            // Version 4 delivers NIN_SELECT / WM_CONTEXTMENU instead of the
            // raw mouse messages, and needs NIF_SHOWTIP to keep the tooltip.
            nid.uFlags = NIF_SHOWTIP;
            nid.uVersion = NOTIFYICON_VERSION_4;
            let v = Shell_NotifyIconW(NIM_SETVERSION, &nid);
            self.log_tray(&format!("NIM_SETVERSION ok={v} err={}", GetLastError()));
            self.tray = true;
        } else {
            self.tray = false;
        }
    }

    /// Append a line to `%APPDATA%\Luma\tray.log`; failures are ignored.
    fn log_tray(&self, msg: &str) {
        use std::io::Write;
        let dir = settings::config_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("tray.log"))
        {
            let _ = writeln!(f, "{} {msg}", settings::now_epoch());
        }
    }

    unsafe fn remove_tray(&mut self) {
        if self.tray {
            let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
            nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = self.hwnd;
            nid.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &nid);
            self.tray = false;
        }
    }

    unsafe fn show_window(&mut self) {
        ShowWindow(self.hwnd, SW_RESTORE);
        ShowWindow(self.hwnd, SW_SHOW);
        SetForegroundWindow(self.hwnd);
    }

    unsafe fn tray_menu_at(&mut self, x: i32, y: i32) {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let show = wide("Show Luma");
        AppendMenuW(menu, MF_STRING, TRAY_SHOW, show.as_ptr());
        let toggle = wide(if self.settings.enabled {
            "Night light: on"
        } else {
            "Night light: off"
        });
        AppendMenuW(
            menu,
            MF_STRING | if self.settings.enabled { MF_CHECKED } else { 0 },
            TRAY_TOGGLE,
            toggle.as_ptr(),
        );
        let paused = self.settings.paused_until > now_epoch();
        let pause = wide(if paused { "Resume now" } else { "Pause 1 hour" });
        AppendMenuW(menu, MF_STRING, TRAY_PAUSE, pause.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        let exit = wide("Exit");
        AppendMenuW(menu, MF_STRING, TRAY_EXIT, exit.as_ptr());
        let mut pt = POINT { x, y };
        if pt.x == 0 && pt.y == 0 {
            GetCursorPos(&mut pt);
        }
        SetForegroundWindow(self.hwnd);
        PostMessageW(self.hwnd, 0, 0, 0);
        let cmd = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            self.hwnd,
            std::ptr::null(),
        );
        DestroyMenu(menu);
        match cmd as usize {
            TRAY_SHOW => self.show_window(),
            TRAY_TOGGLE => self.toggle_enabled(),
            TRAY_PAUSE => self.toggle_pause(),
            TRAY_EXIT => {
                DestroyWindow(self.hwnd);
            }
            _ => {}
        }
    }

    unsafe fn toggle_enabled(&mut self) {
        self.settings.enabled = !self.settings.enabled;
        SendMessageW(
            self.controls.nl_enable,
            BM_SETCHECK,
            if self.settings.enabled { BST_CHECKED } else { 0 },
            0,
        );
        self.mark_dirty();
        self.refresh_night_now();
        self.refresh_footer();
    }

    unsafe fn toggle_pause(&mut self) {
        if self.settings.paused_until > now_epoch() {
            self.settings.paused_until = 0;
        } else {
            self.settings.paused_until = now_epoch() + 3600;
        }
        self.mark_dirty();
        self.refresh_night_now();
        self.refresh_footer();
    }

    // ---------- commands ----------

    unsafe fn on_command(&mut self, id: u16, code: u16) {
        match id {
            C_NAV0 => self.set_page(0),
            C_NAV1 => self.set_page(1),
            C_NAV2 => self.set_page(3),
            C_NAV3 => self.set_page(2),
            C_MON_REFRESH => self.refresh_monitors(),
            C_MON_LIST => {
                if code == LBN_SELCHANGE {
                    let sel = SendMessageW(self.controls.list, LB_GETCURSEL, 0, 0);
                    if sel >= 0 {
                        self.select_monitor(sel as usize);
                    }
                }
            }
            C_VCP_COMBO => {
                if code == CBN_SELCHANGE {
                    let sel = combo_selection(self.controls.vcp_combo);
                    if sel == VCP_CUSTOM {
                        set_text(self.controls.vcp_max, "Enter a custom VCP code and press Use.");
                        SetFocus(self.controls.vcp_custom);
                    } else if sel < VCP_CODES.len() {
                        self.vcp_code = VCP_CODES[sel].0;
                        self.read_vcp();
                    }
                }
            }
            C_VCP_ENUM => {
                if code == CBN_SELCHANGE {
                    let values = enum_values(self.vcp_code);
                    let sel = combo_selection(self.controls.vcp_enum);
                    if let Some((v, _)) = values.get(sel) {
                        self.vcp_write(*v);
                    }
                }
            }
            C_VCP_READ => self.read_vcp(),
            C_VCP_SET => {
                let text = hwnd_text(self.controls.vcp_edit);
                match parse_number(&text) {
                    Some(v) => self.vcp_write(v),
                    None => {
                        self.set_footer("Type a value in the Set value box, then press Set.");
                        SetFocus(self.controls.vcp_edit);
                    }
                }
            }
            C_VCP_USE => {
                let text = hwnd_text(self.controls.vcp_custom);
                match parse_number(&text) {
                    Some(v) if v <= 255 => {
                        self.vcp_code = v as u8;
                        self.vcp_enum_visible = false;
                        self.read_vcp();
                    }
                    _ => set_text(self.controls.vcp_max, "VCP codes are 0x00–0xFF (e.g. 0x10)."),
                }
            }
            C_NL_ENABLE => {
                let on = !self.settings.enabled;
                self.settings.enabled = on;
                InvalidateRect(self.controls.nl_enable, std::ptr::null(), 1);
                self.mark_dirty();
                self.refresh_night_now();
                self.refresh_footer();
            }
            C_NL_MODE => {
                self.settings.mode = match combo_selection(self.controls.nl_mode) {
                    1 => "custom".into(),
                    2 => "always".into(),
                    _ => "sun".into(),
                };
                self.mark_dirty();
                self.refresh_night_now();
            }
            C_NL_LOCATION => {
                let sel = combo_selection(self.controls.nl_location);
                match CITIES.get(sel) {
                    Some((name, lat, lon, tz)) => {
                        self.set_solar_location(*lat, *lon, *tz);
                        self.set_footer(&format!(
                            "Location set to {}. Sunrise and sunset follow these coordinates.",
                            name
                        ));
                    }
                    None => {
                        SetFocus(self.controls.lat);
                        self.set_footer(
                            "Custom coordinates: type a latitude and longitude, then press Apply schedule.",
                        );
                    }
                }
            }
            C_NL_TZ => {
                let tz = TZ_MIN + combo_selection(self.controls.tz) as f64 * 0.5;
                self.set_solar_location(self.settings.lat, self.settings.lon, tz);
                self.set_footer(&format!(
                    "Time zone set to {}. Schedule and solar times are local.",
                    tz_label(tz)
                ));
            }
            C_NL_USE_TZ => {
                let tz = (current_tz_offset_minutes() as f64 / 60.0 * 2.0).round() / 2.0;
                self.set_solar_location(self.settings.lat, self.settings.lon, tz);
                self.set_footer(&format!(
                    "Time zone now follows Windows: {}.",
                    tz_label(tz)
                ));
            }
            C_NL_WEEKEND => {
                self.settings.weekend = !self.settings.weekend;
                InvalidateRect(self.controls.weekend, std::ptr::null(), 1);
                self.mark_dirty();
                self.refresh_night_now();
            }
            C_NL_PAUSE => self.toggle_pause(),
            C_NL_APPLY => self.apply_schedule_edits(),
            C_NL_RESET_TINT => {
                self.settings.temp = settings::DEFAULT_TEMP;
                self.settings.strength = settings::DEFAULT_STRENGTH;
                self.settings.fade = settings::DEFAULT_FADE;
                set_slider(self.controls.temp, self.settings.temp as i32);
                set_slider(self.controls.strength, self.settings.strength as i32);
                set_slider(self.controls.fade, self.settings.fade as i32);
                self.update_night_control_labels();
                self.refresh_night_now();
                self.mark_dirty();
                self.set_footer("Tint reset to defaults: 3400 K, 55%, 30 min fade.");
            }
            C_DIM_ENABLE => {
                self.settings.auto_dim = !self.settings.auto_dim;
                InvalidateRect(self.controls.dim_enable, std::ptr::null(), 1);
                self.mark_dirty();
                self.dim_last_pct = None;
                self.refresh_dim_labels();
                if self.settings.auto_dim {
                    let curve = self.dim_curve();
                    if dim::valid(&curve) {
                        let (pct, _) = dim::target_phase(&curve, local_minute());
                        self.apply_auto_dim(pct, true);
                    }
                }
                self.set_footer(if self.settings.auto_dim {
                    "Auto dim on."
                } else {
                    "Auto dim off."
                });
            }
            C_DIM_APPLY => self.apply_dim_edits(),
            C_AUTOSTART => {
                let want = !self.settings.autostart;
                self.settings.autostart = want;
                InvalidateRect(self.controls.autostart, std::ptr::null(), 1);
                if !set_autostart(want) {
                    self.settings.autostart = false;
                    InvalidateRect(self.controls.autostart, std::ptr::null(), 1);
                    message_box(
                        self.hwnd,
                        "Could not update the Windows startup entry.",
                        "Luma",
                        0x10,
                    );
                }
                self.mark_dirty();
            }
            C_MINIMIZE => {
                self.settings.minimize_tray = !self.settings.minimize_tray;
                InvalidateRect(self.controls.minimize, std::ptr::null(), 1);
                self.mark_dirty();
            }
            C_DARK_MODE => {
                self.settings.dark = !self.settings.dark;
                InvalidateRect(self.controls.dark_mode, std::ptr::null(), 1);
                self.apply_theme();
                self.mark_dirty();
                set_text(
                    self.controls.footer,
                    if self.settings.dark {
                        "Dark theme enabled."
                    } else {
                        "Light theme enabled."
                    },
                );
            }
            C_OPEN_FOLDER => {
                let dir = config_dir();
                let _ = std::fs::create_dir_all(&dir);
                let _ = Command::new("explorer.exe")
                    .arg(dir)
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn();
            }
            C_RESET => {
                let r = message_box(
                    self.hwnd,
                    "Reset all Luma settings to defaults?",
                    "Luma",
                    0x4 | 0x20,
                );
                if r == 6 {
                    let tz = current_tz_offset_minutes() as f64 / 60.0;
                    self.settings = Settings::default();
                    self.settings.tz = tz;
                    self.settings.selected = 0;
                    self.selected = 0;
                    self.settings.save();
                    self.dirty = false;
                    self.populate_settings_ui();
                    self.refresh_night_now();
                    self.refresh_footer();
                    self.set_footer("Settings reset to defaults.");
                }
            }
            _ => {}
        }
    }

    unsafe fn apply_schedule_edits(&mut self) {
        let lat_s = hwnd_text(self.controls.lat);
        let lon_s = hwnd_text(self.controls.lon);
        let lat: f64 = match lat_s.trim().parse() {
            Ok(v) if (-90.0..=90.0).contains(&v) => v,
            _ => return self.schedule_error("Latitude must be between -90 and 90."),
        };
        let lon: f64 = match lon_s.trim().parse() {
            Ok(v) if (-180.0..=180.0).contains(&v) => v,
            _ => return self.schedule_error("Longitude must be between -180 and 180."),
        };
        let tz = TZ_MIN + combo_selection(self.controls.tz) as f64 * 0.5;
        let wd_start = normalize_or_error(hwnd_text(self.controls.wd_start));
        let wd_end = normalize_or_error(hwnd_text(self.controls.wd_end));
        let we_start = normalize_or_error(hwnd_text(self.controls.we_start));
        let we_end = normalize_or_error(hwnd_text(self.controls.we_end));
        let (Some(wd_start), Some(wd_end), Some(we_start), Some(we_end)) =
            (wd_start, wd_end, we_start, we_end)
        else {
            return self.schedule_error("Schedule times must be 24-hour HH:MM, e.g. 19:30.");
        };
        if parse_hhmm(&wd_start).is_none() || parse_hhmm(&wd_end).is_none() {
            return self.schedule_error("Schedule times must be 24-hour HH:MM, e.g. 19:30.");
        }
        self.settings.lat = lat;
        self.settings.lon = lon;
        self.settings.tz = tz;
        set_combo_selection(
            self.controls.nl_location,
            city_selection(lat, lon).unwrap_or(CITIES.len()),
        );
        self.settings.wd_start = wd_start;
        self.settings.wd_end = wd_end;
        self.settings.we_start = we_start;
        self.settings.we_end = we_end;
        self.settings.save();
        self.dirty = false;
        self.refresh_night_now();
        self.set_footer("Night-light schedule saved.");
        let now = night::now_info();
        self.update_night_labels(
            &night::compute(&self.settings, now, now_epoch()),
            now,
        );
    }

    unsafe fn schedule_error(&mut self, text: &str) {
        message_box(self.hwnd, text, "Luma", 0x10);
    }

    unsafe fn on_hscroll(&mut self, code: u32, hwnd: HWND) {
        if hwnd == self.controls.bright {
            let pos = SendMessageW(self.controls.bright, TBM_GETPOS, 0, 0);
            set_text(self.controls.bright_label, &format!("{}%", pos));
            if code == TB_ENDTRACK || code == TB_THUMBPOSITION {
                self.set_brightness_from_slider();
            }
        } else if hwnd == self.controls.panel {
            let pos = SendMessageW(self.controls.panel, TBM_GETPOS, 0, 0);
            set_text(self.controls.panel_label, &format!("{}%", pos));
            if code == TB_ENDTRACK || code == TB_THUMBPOSITION {
                self.set_panel_from_slider();
            }
        } else if hwnd == self.controls.vcp_slider {
            let pos = SendMessageW(self.controls.vcp_slider, TBM_GETPOS, 0, 0);
            set_text(
                self.controls.vcp_value,
                &format!("{} / {}", pos, self.vcp_max),
            );
            set_text(self.controls.vcp_edit, &pos.to_string());
            if code == TB_ENDTRACK || code == TB_THUMBPOSITION {
                self.vcp_write(pos as u32);
            }
        } else if hwnd == self.controls.temp {
            let pos = SendMessageW(self.controls.temp, TBM_GETPOS, 0, 0).clamp(1000, 6500);
            self.settings.temp = pos as u32;
            set_text(self.controls.temp_label, &format!("{} K", pos));
            self.refresh_night_now();
            self.refresh_tint_preview();
            if code == TB_ENDTRACK {
                self.mark_dirty();
            }
        } else if hwnd == self.controls.strength {
            let pos = SendMessageW(self.controls.strength, TBM_GETPOS, 0, 0).clamp(0, 100);
            self.settings.strength = pos as u32;
            set_text(self.controls.strength_label, &format!("{}%", pos));
            self.refresh_night_now();
            self.refresh_tint_preview();
            if code == TB_ENDTRACK {
                self.mark_dirty();
            }
        } else if hwnd == self.controls.fade {
            let pos = SendMessageW(self.controls.fade, TBM_GETPOS, 0, 0).clamp(0, 240);
            self.settings.fade = pos as u32;
            set_text(self.controls.fade_label, &format!("{} min", pos));
            if code == TB_ENDTRACK {
                self.mark_dirty();
            }
        }
    }

    unsafe fn on_notify(&mut self, lparam: LPARAM) {
        let _ = lparam;
    }

    unsafe fn on_destroy(&mut self) {
        self.remove_tray();
        UnregisterHotKey(self.hwnd, 1);
        UnregisterHotKey(self.hwnd, 2);
        UnregisterHotKey(self.hwnd, 3);
        self.settings.save();
        self.restore_original_gamma();
        if !self.fonts.base.is_null() {
            DeleteObject(self.fonts.base);
            DeleteObject(self.fonts.bold);
            DeleteObject(self.fonts.title);
            DeleteObject(self.fonts.heading);
            DeleteObject(self.fonts.small);
        }
        if !self.br_bg.is_null() {
            DeleteObject(self.br_bg as HANDLE);
        }
        if !self.br_card.is_null() {
            DeleteObject(self.br_card as HANDLE);
        }
        if !self.mutex.is_null() {
            CloseHandle(self.mutex);
        }
    }
}

fn vcp_name(code: u8) -> String {
    VCP_CODES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| format!("VCP 0x{code:02X}"))
}

fn enum_values(code: u8) -> &'static [(u32, &'static str)] {
    match code {
        0x60 => &[
            (1, "Analog"),
            (3, "DVI"),
            (4, "HDMI"),
            (5, "HDMI 2"),
            (15, "DisplayPort"),
            (16, "DisplayPort 2"),
            (17, "HDMI 3"),
            (18, "HDMI 4"),
        ],
        0xD6 => &[
            (1, "On"),
            (2, "Standby"),
            (3, "Suspend"),
            (4, "Off"),
            (5, "Hard off"),
        ],
        0x14 => &[
            (1, "sRGB"),
            (2, "Native"),
            (3, "4000 K"),
            (4, "5000 K"),
            (5, "6500 K"),
            (6, "7500 K"),
            (7, "8200 K"),
            (8, "9300 K"),
            (9, "10000 K"),
            (10, "User 1"),
            (11, "User 2"),
            (12, "User 3"),
        ],
        _ => &[],
    }
}

fn parse_number(s: &str) -> Option<u32> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn normalize_or_error(s: String) -> Option<String> {
    normalize_time(&s)
}

fn fmt_tz(tz: f64) -> String {
    if (tz - tz.round()).abs() < 0.01 {
        format!("{}", tz.round() as i32)
    } else {
        format!("{:.1}", tz)
    }
}

fn local_minute() -> u16 {
    let st = unsafe { local_now() };
    (st.wHour * 60 + st.wMinute) as u16
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

unsafe fn set_slider(hwnd: HWND, value: i32) {
    SendMessageW(hwnd, TBM_SETPOS, 1, value as isize);
}

unsafe fn set_slider_range(hwnd: HWND, min: i32, max: i32) {
    SendMessageW(
        hwnd,
        TBM_SETRANGE,
        1,
        ((max << 16) | (min & 0xFFFF)) as isize,
    );
}

// ---------- custom slider ----------

const SLIDER_CLASS: &str = "LumaSlider";

struct SliderState {
    min: i32,
    max: i32,
    pos: i32,
}

unsafe fn slider_state(hwnd: HWND) -> *mut SliderState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SliderState
}

unsafe fn slider_round(hdc: HDC, rc: &RECT, radius: i32, color: u32) {
    let brush = CreateSolidBrush(color);
    let pen = CreatePen(0, 1, color);
    let old_brush = SelectObject(hdc, brush as HANDLE);
    let old_pen = SelectObject(hdc, pen as HANDLE);
    RoundRect(
        hdc,
        rc.left,
        rc.top,
        rc.right,
        rc.bottom,
        radius * 2,
        radius * 2,
    );
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    DeleteObject(brush as HANDLE);
    DeleteObject(pen as HANDLE);
}

unsafe fn slider_paint(hwnd: HWND) {
    let st = slider_state(hwnd);
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let brush = CreateSolidBrush(pal().card);
    FillRect(hdc, &rc, brush);
    DeleteObject(brush as HANDLE);
    if !st.is_null() {
        let enabled = IsWindowEnabled(hwnd) != 0;
        let cy = (rc.bottom - rc.top) / 2;
        let left = 10;
        let right = (rc.right - 10).max(left + 1);
        let track = RECT {
            left,
            top: cy - 3,
            right,
            bottom: cy + 3,
        };
        slider_round(
            hdc,
            &track,
            3,
            if enabled {
                pal().border
            } else {
                pal().disabled_bg
            },
        );
        let span = (right - left).max(1) as f32;
        let range = ((*st).max - (*st).min).max(1) as f32;
        let t = (((*st).pos - (*st).min) as f32 / range).clamp(0.0, 1.0);
        let tx = left + (t * span).round() as i32;
        if tx > left {
            let filled = RECT {
                left,
                top: cy - 3,
                right: tx,
                bottom: cy + 3,
            };
            slider_round(
                hdc,
                &filled,
                3,
                if enabled {
                    pal().accent
                } else {
                    pal().disabled_text
                },
            );
        }
        let thumb = RECT {
            left: tx - 8,
            top: cy - 8,
            right: tx + 8,
            bottom: cy + 8,
        };
        let tb = CreateSolidBrush(pal().card);
        let tp = CreatePen(
            0,
            2,
            if enabled {
                pal().accent
            } else {
                pal().muted
            },
        );
        let ob = SelectObject(hdc, tb as HANDLE);
        let op = SelectObject(hdc, tp as HANDLE);
        Ellipse(hdc, thumb.left, thumb.top, thumb.right, thumb.bottom);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(tb as HANDLE);
        DeleteObject(tp as HANDLE);
        if GetFocus() == hwnd {
            let ring = RECT {
                left: thumb.left - 3,
                top: thumb.top - 3,
                right: thumb.right + 3,
                bottom: thumb.bottom + 3,
            };
            let rp = CreatePen(0, 1, pal().accent_dark);
            let ro = SelectObject(hdc, rp as HANDLE);
            let rb = SelectObject(hdc, GetStockObject(NULL_BRUSH));
            Ellipse(hdc, ring.left, ring.top, ring.right, ring.bottom);
            SelectObject(hdc, ro);
            SelectObject(hdc, rb);
            DeleteObject(rp as HANDLE);
        }
    }
    EndPaint(hwnd, &ps);
}

unsafe fn slider_set_from_x(hwnd: HWND, x: i32) {
    let st = slider_state(hwnd);
    if st.is_null() {
        return;
    }
    let mut rc: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rc);
    let left = 10;
    let span = ((rc.right - 20).max(1)) as f32;
    let t = ((x - left) as f32 / span).clamp(0.0, 1.0);
    let range = (*st).max - (*st).min;
    (*st).pos = (*st).min + (t * range as f32).round() as i32;
    InvalidateRect(hwnd, std::ptr::null(), 0);
}

unsafe fn slider_notify(hwnd: HWND, code: u32) {
    let st = slider_state(hwnd);
    if st.is_null() {
        return;
    }
    let parent = GetParent(hwnd);
    if !parent.is_null() {
        SendMessageW(
            parent,
            WM_HSCROLL,
            (((*st).pos as usize) << 16) | code as usize,
            hwnd as isize,
        );
    }
}

unsafe extern "system" fn slider_wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let st = Box::new(SliderState {
                min: 0,
                max: 100,
                pos: 0,
            });
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(st) as isize);
            return 0;
        }
        WM_NCDESTROY => {
            let st = slider_state(hwnd);
            if !st.is_null() {
                drop(Box::from_raw(st));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
        }
        WM_PAINT => {
            slider_paint(hwnd);
            return 0;
        }
        WM_ERASEBKGND => return 1,
        WM_GETDLGCODE => return DLGC_WANTARROWS,
        WM_LBUTTONDOWN => {
            SetFocus(hwnd);
            SetCapture(hwnd);
            let x = (l as u32 & 0xFFFF) as i16 as i32;
            slider_set_from_x(hwnd, x);
            slider_notify(hwnd, TB_THUMBTRACK);
            return 0;
        }
        WM_MOUSEMOVE => {
            if GetCapture() == hwnd {
                let x = (l as u32 & 0xFFFF) as i16 as i32;
                slider_set_from_x(hwnd, x);
                slider_notify(hwnd, TB_THUMBTRACK);
            }
            return 0;
        }
        WM_LBUTTONUP => {
            if GetCapture() == hwnd {
                ReleaseCapture();
                slider_notify(hwnd, TB_ENDTRACK);
            }
            return 0;
        }
        WM_KEYDOWN => {
            let st = slider_state(hwnd);
            if st.is_null() {
                return 0;
            }
            let step = if GetKeyState(VK_SHIFT as i32) < 0 { 10 } else { 1 };
            match w as u32 {
                VK_LEFT | VK_DOWN => (*st).pos = ((*st).pos - step).max((*st).min),
                VK_RIGHT | VK_UP => (*st).pos = ((*st).pos + step).min((*st).max),
                _ => return DefWindowProcW(hwnd, msg, w, l),
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
            slider_notify(hwnd, TB_ENDTRACK);
            return 0;
        }
        WM_ENABLE => {
            InvalidateRect(hwnd, std::ptr::null(), 1);
            return 0;
        }
        TBM_SETRANGE => {
            let st = slider_state(hwnd);
            if st.is_null() {
                return 0;
            }
            (*st).min = ((l as u32) & 0xFFFF) as i16 as i32;
            (*st).max = (((l as u32) >> 16) & 0xFFFF) as i16 as i32;
            if (*st).max <= (*st).min {
                (*st).max = (*st).min + 1;
            }
            (*st).pos = (*st).pos.clamp((*st).min, (*st).max);
            InvalidateRect(hwnd, std::ptr::null(), 0);
            return 0;
        }
        TBM_SETPOS => {
            let st = slider_state(hwnd);
            if st.is_null() {
                return 0;
            }
            (*st).pos = (l as i32).clamp((*st).min, (*st).max);
            InvalidateRect(hwnd, std::ptr::null(), 0);
            return 0;
        }
        TBM_GETPOS => {
            let st = slider_state(hwnd);
            return if st.is_null() { 0 } else { (*st).pos as LRESULT };
        }
        TBM_GETRANGEMIN => {
            let st = slider_state(hwnd);
            return if st.is_null() { 0 } else { (*st).min as LRESULT };
        }
        TBM_GETRANGEMAX => {
            let st = slider_state(hwnd);
            return if st.is_null() { 0 } else { (*st).max as LRESULT };
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w, l)
}

unsafe fn register_slider_class(hinst: HINSTANCE) {
    let name = wide(SLIDER_CLASS);
    let cls = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(slider_wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: null_mut(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszMenuName: std::ptr::null(),
        lpszClassName: name.as_ptr(),
        hIconSm: null_mut(),
    };
    RegisterClassExW(&cls);
}

unsafe fn message_box(hwnd: HWND, text: &str, title: &str, flags: u32) -> i32 {
    let t = wide(text);
    let c = wide(title);
    MessageBoxW(hwnd, t.as_ptr(), c.as_ptr(), flags)
}

fn set_autostart(enable: bool) -> bool {
    unsafe {
        let sub = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
        let mut key: HKEY = null_mut();
        let mut disp: u32 = 0;
        let r = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            0,
            KEY_SET_VALUE,
            std::ptr::null(),
            &mut key,
            &mut disp,
        );
        if r != 0 || key.is_null() {
            return false;
        }
        let name = wide("Luma");
        let ok = if enable {
            let exe = std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let val = wide(&format!("\"{}\"", exe));
            let bytes = (val.len() * 2) as u32;
            RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                val.as_ptr() as *const u8,
                bytes,
            ) == 0
        } else {
            let r = RegDeleteValueW(key, name.as_ptr());
            r == 0 || r == 2
        };
        RegCloseKey(key);
        ok
    }
}

unsafe fn is_first_instance() -> Option<HANDLE> {
    let name = wide(MUTEX_NAME);
    let h = CreateMutexW(std::ptr::null_mut(), 1, name.as_ptr());
    let err = GetLastError();
    if h.is_null() {
        // If another instance created the mutex at a different integrity
        // level we may be denied access to it. Treat that as "already
        // running" so we surface the existing window instead of opening a
        // second copy that fights over monitor handles.
        if err == ERROR_ACCESS_DENIED {
            return None;
        }
        return Some(null_mut());
    }
    if err == ERROR_ALREADY_EXISTS {
        CloseHandle(h);
        None
    } else {
        Some(h)
    }
}

fn make_icon(hinst: HINSTANCE) -> HICON {
    unsafe {
        let mut xor = [0u32; 32 * 32];
        let mut and = [0u8; 32 * 4];
        for y in 0..32usize {
            for x in 0..32usize {
                match icon_pixel(x, y) {
                    Some(px) => xor[y * 32 + x] = px,
                    None => and[y * 4 + x / 8] |= 1 << (7 - (x % 8)),
                }
            }
        }
        CreateIcon(
            hinst,
            32,
            32,
            1,
            32,
            and.as_ptr(),
            xor.as_ptr() as *const u8,
        )
    }
}

/// One pixel of the tray/app icon: `None` where the round mark ends, otherwise
/// `0xAARRGGBB`. The alpha byte must stay opaque — the shell blends icons, so
/// a zero alpha channel draws a fully transparent (and therefore invisible)
/// tray slot.
fn icon_pixel(x: usize, y: usize) -> Option<u32> {
    let dx = x as f64 - 15.5;
    let dy = y as f64 - 15.5;
    if (dx * dx + dy * dy).sqrt() > 13.5 {
        return None;
    }
    let t = (y as f64 / 31.0).clamp(0.0, 1.0);
    let r = 255u32;
    let g = (206.0 - 76.0 * t) as u32;
    let b = (120.0 - 88.0 * t) as u32;
    Some(0xFF00_0000 | (r << 16) | (g << 8) | b)
}

unsafe fn fill_color(hdc: HDC, rc: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    FillRect(hdc, rc, brush);
    DeleteObject(brush as HANDLE);
}

unsafe fn fill_ellipse(hdc: HDC, rc: &RECT, color: u32) {
    let brush = CreateSolidBrush(color);
    let pen = CreatePen(PS_SOLID, 1, color);
    let old_brush = SelectObject(hdc, brush as HANDLE);
    let old_pen = SelectObject(hdc, pen as HANDLE);
    Ellipse(hdc, rc.left, rc.top, rc.right, rc.bottom);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    DeleteObject(brush as HANDLE);
    DeleteObject(pen as HANDLE);
}

unsafe fn round_fill(hdc: HDC, rc: &RECT, radius: i32, fill: u32, border: u32) {
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, brush as HANDLE);
    let old_pen = SelectObject(hdc, pen as HANDLE);
    RoundRect(
        hdc,
        rc.left,
        rc.top,
        rc.right,
        rc.bottom,
        radius * 2,
        radius * 2,
    );
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    DeleteObject(brush as HANDLE);
    DeleteObject(pen as HANDLE);
}

unsafe fn draw_text(hdc: HDC, text: &str, rc: &mut RECT, font: HFONT, color: u32, fmt: u32) {
    let old = SelectObject(hdc, font as HANDLE);
    SetBkMode(hdc, TRANSPARENT);
    SetTextColor(hdc, color);
    let w = wide(text);
    DrawTextW(hdc, w.as_ptr(), -1, rc, fmt | DT_NOPREFIX);
    SelectObject(hdc, old);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let taskbar_created = TASKBAR_CREATED.load(Ordering::Relaxed);
    if taskbar_created != 0 && msg == taskbar_created {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
        if !ptr.is_null() {
            let app = &mut *ptr;
            app.tray = false;
            app.add_tray();
        }
        return 0;
    }
    match msg {
        WM_NCCREATE => {
            let cs = l as *const CREATESTRUCTW;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
            return 1;
        }
        WM_CREATE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                (*ptr).on_create(hwnd);
            }
            return 0;
        }
        WM_DESTROY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let mut app = Box::from_raw(ptr);
                app.on_destroy();
            }
            PostQuitMessage(0);
            return 0;
        }
        WM_ERASEBKGND => {
            let hdc = w as HDC;
            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc);
            fill_color(hdc, &rc, pal().bg);
            return 1;
        }
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            EndPaint(hwnd, &ps);
            let _ = hdc;
            return 0;
        }
        WM_DRAWITEM => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() && l != 0 {
                let d = &*(l as *const DRAWITEMSTRUCT);
                (*ptr).on_drawitem(d);
            }
            return 1;
        }
        WM_CTLCOLOREDIT => {
            let hdc = w as HDC;
            SetBkMode(hdc, 2);
            SetBkColor(hdc, pal().well);
            SetTextColor(hdc, pal().text);
            let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !app.is_null() && !(*app).br_well.is_null() {
                return (*app).br_well as LRESULT;
            }
            return GetStockObject(WHITE_BRUSH) as LRESULT;
        }
        WM_CTLCOLORLISTBOX => {
            let hdc = w as HDC;
            SetBkMode(hdc, 2);
            SetBkColor(hdc, pal().well);
            SetTextColor(hdc, pal().text);
            let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !app.is_null() && !(*app).br_well.is_null() {
                return (*app).br_well as LRESULT;
            }
        }
        WM_CTLCOLORSTATIC => {
            let hdc = w as HDC;
            let ctl = l as HWND;
            let mut buf = [0u16; 64];
            let n = GetClassNameW(ctl, buf.as_mut_ptr(), buf.len() as i32);
            let class = from_wide(&buf[..n.max(0) as usize]);
            if class == "msctls_trackbar32" {
                SetBkColor(hdc, pal().card);
                return GetStockObject(WHITE_BRUSH) as LRESULT;
            }
            // Disabled edits and list controls send WM_CTLCOLORSTATIC, not
            // WM_CTLCOLOREDIT. They still need an opaque field background.
            if class == "Edit" || class == "ComboBox" || class == "ListBox" {
                SetBkMode(hdc, 2);
                SetBkColor(hdc, pal().well);
                SetTextColor(hdc, pal().text);
                let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !app.is_null() && !(*app).br_well.is_null() {
                    return (*app).br_well as LRESULT;
                }
                return GetStockObject(WHITE_BRUSH) as LRESULT;
            }
            if class == "Button" {
                SetBkMode(hdc, 2);
                SetBkColor(hdc, pal().card);
                SetTextColor(hdc, pal().text);
                let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !app.is_null() {
                    return (*app).br_card as LRESULT;
                }
            }
            let flag = GetWindowLongPtrW(ctl, GWLP_USERDATA);
            if flag >= 2 && flag <= 4 {
                // Opaque label: 2 = page background, 3 = card, 4 = muted on card.
                // Erases old text so changing status strings never leaves ghosts.
                let card = flag >= 3;
                SetBkMode(hdc, 2);
                SetBkColor(hdc, if card { pal().card } else { pal().bg });
                SetTextColor(hdc, if flag == 3 { pal().text } else { pal().muted });
                let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !app.is_null() {
                    return if card {
                        (*app).br_card as LRESULT
                    } else {
                        (*app).br_bg as LRESULT
                    };
                }
            }
            SetBkMode(hdc, TRANSPARENT);
            let muted = flag != 0;
            SetTextColor(hdc, if muted { pal().muted } else { pal().text });
            return GetStockObject(NULL_BRUSH) as LRESULT;
        }
        WM_CTLCOLORBTN => {
            let hdc = w as HDC;
            SetBkMode(hdc, 2);
            SetBkColor(hdc, pal().card);
            let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !app.is_null() {
                return (*app).br_card as LRESULT;
            }
            return GetStockObject(WHITE_BRUSH) as LRESULT;
        }
        WM_COMMAND => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                let id = (w & 0xFFFF) as u16;
                let code = ((w >> 16) & 0xFFFF) as u16;
                (*ptr).on_command(id, code);
            }
            return 0;
        }
        WM_HSCROLL => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                (*ptr).on_hscroll((w & 0xFFFF) as u32, l as HWND);
            }
            return 0;
        }
        WM_NOTIFY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                (*ptr).on_notify(l);
            }
            return 0;
        }
        WM_TIMER => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                (*ptr).tick();
            }
            return 0;
        }
        WM_HOTKEY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                let app = &mut *ptr;
                if w == 1 {
                    app.toggle_enabled();
                } else if w == 2 || w == 3 {
                    app.adjust_brightness_hotkey(if w == 2 { 5 } else { -5 });
                }
            }
            return 0;
        }
        WM_TRAY => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                let app = &mut *ptr;
                // NOTIFYICON_VERSION_4 packs the anchor point into wParam and
                // the event itself into lParam.
                let x = (w as u32 & 0xFFFF) as u16 as i16 as i32;
                let y = ((w as u32 >> 16) & 0xFFFF) as u16 as i16 as i32;
                match l as u32 {
                    WM_CONTEXTMENU => app.tray_menu_at(x, y),
                    NIN_SELECT | NIN_KEYSELECT | WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                        app.show_window()
                    }
                    WM_RBUTTONUP => app.tray_menu_at(0, 0),
                    _ => {}
                }
            }
            return 0;
        }
        WM_CLOSE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                let app = &mut *ptr;
                if app.settings.minimize_tray {
                    if !app.tray {
                        app.add_tray();
                    }
                    if app.tray {
                        ShowWindow(hwnd, SW_HIDE);
                    } else {
                        message_box(
                            hwnd,
                            "Luma could not add a tray icon, so the window stays open. \
                             Choose a different close behaviour in Settings, or restart Explorer.",
                            "Luma",
                            0x30,
                        );
                    }
                } else {
                    DestroyWindow(hwnd);
                }
            }
            return 0;
        }
        WM_DISPLAYCHANGE => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
            if !ptr.is_null() {
                let app = &mut *ptr;
                app.refresh_monitors();
                app.refresh_night_now();
            }
            return 0;
        }
        WM_POWERBROADCAST => {
            if w == PBT_APMRESUMEAUTOMATIC as usize || w == PBT_APMRESUMESUSPEND as usize {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
                if !ptr.is_null() {
                    (*ptr).refresh_night_now();
                }
            }
            return 1;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w, l)
}

impl App {
    unsafe fn adjust_brightness_hotkey(&mut self, delta: i32) {
        if self.displays.is_empty() {
            return;
        }
        let idx = self.selected.min(self.displays.len() - 1);
        let Some(b) = self.displays[idx].brightness else {
            return;
        };
        let pct = (b.current.saturating_mul(100) / b.max.max(1)) as i32;
        let new_pct = (pct + delta).clamp(0, 100) as u32;
        let raw = new_pct.saturating_mul(b.max.max(1)) / 100;
        if self.displays[idx].set_brightness(raw) {
            set_slider(self.controls.bright, new_pct as i32);
            set_text(self.controls.bright_label, &format!("{}%", new_pct));
            self.refresh_footer();
        }
    }
}

fn show_existing_instance() {
    unsafe {
        let cls = wide(CLASS_NAME);
        let h = FindWindowW(cls.as_ptr(), std::ptr::null());
        if !h.is_null() {
            ShowWindow(h, SW_RESTORE);
            ShowWindow(h, SW_SHOW);
            SetForegroundWindow(h);
        }
    }
}

unsafe fn run() -> i32 {
    let hinst = GetModuleHandleW(std::ptr::null());
    let mut icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_WIN95_CLASSES,
    };
    InitCommonControlsEx(&icc);
    // RichEdit 4.1 gives us a single-line field that honours a dark
    // background (EM_SETBKGNDCOLOR); native EDIT ignores it here.
    let msftedit = wide("Msftedit.dll");
    let _richedit = LoadLibraryW(msftedit.as_ptr());
    register_slider_class(hinst);

    let icon = make_icon(hinst);
    let taskbar_msg = wide("TaskbarCreated");
    TASKBAR_CREATED.store(RegisterWindowMessageW(taskbar_msg.as_ptr()), Ordering::Relaxed);
    let class_w = wide(CLASS_NAME);
    let cls = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        hIcon: icon,
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: CreateSolidBrush(pal().bg),
        lpszMenuName: std::ptr::null(),
        lpszClassName: class_w.as_ptr(),
        hIconSm: icon,
    };
    if RegisterClassExW(&cls) == 0 {
        message_box(null_mut(), "Could not register the Luma window class.", "Luma", 0x10);
        return 1;
    }

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: CLIENT_W,
        bottom: CLIENT_H,
    };
    let win_style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
    AdjustWindowRectEx(&mut rect, win_style, 0, 0);
    let ww = rect.right - rect.left;
    let wh = rect.bottom - rect.top;
    let sw = GetSystemMetrics(0);
    let sh = GetSystemMetrics(1);
    let x = (sw - ww) / 2;
    let y = (sh - wh) / 2;

    let Some(mutex) = is_first_instance() else {
        show_existing_instance();
        return 0;
    };

    let app = Box::new(App::new(hinst, mutex));
    let app_ptr = Box::into_raw(app) as *mut c_void;
    let title = wide(WINDOW_TITLE);
    let hwnd = CreateWindowExW(
        0,
        class_w.as_ptr(),
        title.as_ptr(),
        win_style,
        x,
        y,
        ww,
        wh,
        null_mut(),
        null_mut(),
        hinst,
        app_ptr,
    );
    if hwnd.is_null() {
        message_box(null_mut(), "Could not create the Luma window.", "Luma", 0x10);
        return 1;
    }
    ShowWindow(hwnd, SW_SHOW);
    UpdateWindow(hwnd);

    let mut msg: MSG = std::mem::zeroed();
    loop {
        let r = GetMessageW(&mut msg, null_mut(), 0, 0);
        if r <= 0 {
            break;
        }
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    0
}

#[cfg(test)]
mod tray_icon_tests {
    use super::icon_pixel;

    #[test]
    fn tray_icon_pixels_are_opaque() {
        let px = icon_pixel(16, 16).expect("the centre of the mark must be drawn");
        assert_eq!(
            px >> 24,
            0xFF,
            "alpha must be opaque, otherwise the shell draws an invisible tray slot"
        );
        assert!(icon_pixel(0, 0).is_none(), "corners stay transparent");
        assert!(icon_pixel(31, 31).is_none(), "corners stay transparent");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--diagnose") {
        monitors::diagnose();
        return;
    }
    unsafe {
        let code = run();
        std::process::exit(code);
    }
}


