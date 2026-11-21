use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct Settings {
    pub enabled: bool,
    pub mode: String, // sun | custom | always
    pub temp: u32,
    pub strength: u32,
    pub fade: u32,
    pub lat: f64,
    pub lon: f64,
    pub tz: f64,
    pub wd_start: String,
    pub wd_end: String,
    pub weekend: bool,
    pub we_start: String,
    pub we_end: String,
    pub sunset_offset: i32,
    pub sunrise_offset: i32,
    pub minimize_tray: bool,
    pub autostart: bool,
    pub dark: bool,
    pub selected: usize,
    pub paused_until: u64,
    // Time-based brightness curve.
    pub auto_dim: bool,
    pub dim_day: String,
    pub dim_evening: String,
    pub dim_night: String,
    pub dim_day_pct: u32,
    pub dim_evening_pct: u32,
    pub dim_night_pct: u32,
    pub dim_fade: u32,
}

pub const DEFAULT_TEMP: u32 = 3400;
pub const DEFAULT_STRENGTH: u32 = 55;
pub const DEFAULT_FADE: u32 = 30;

/// Bumped when a key changes meaning or is removed. Older files load with
/// defaults for anything they do not contain.
pub const SETTINGS_SCHEMA: u32 = 2;

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: "sun".into(),
            temp: DEFAULT_TEMP,
            strength: DEFAULT_STRENGTH,
            fade: DEFAULT_FADE,
            lat: 14.6760,
            lon: 121.0437,
            tz: 8.0,
            wd_start: "19:00".into(),
            wd_end: "07:00".into(),
            weekend: false,
            we_start: "20:00".into(),
            we_end: "08:00".into(),
            sunset_offset: 0,
            sunrise_offset: 0,
            minimize_tray: true,
            autostart: false,
            dark: false,
            selected: 0,
            paused_until: 0,
            auto_dim: false,
            dim_day: "07:00".into(),
            dim_evening: "21:00".into(),
            dim_night: "23:00".into(),
            dim_day_pct: 100,
            dim_evening_pct: 75,
            dim_night_pct: 35,
            dim_fade: 20,
        }
    }
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Luma")
}

pub fn config_path() -> PathBuf {
    config_dir().join("settings.ini")
}

pub fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn parse_bool(s: &str) -> bool {
    matches!(s.trim(), "1" | "true" | "yes" | "on")
}

impl Settings {
    pub fn load() -> Self {
        let mut s = Settings::default();
        let text = match fs::read_to_string(config_path()) {
            Ok(t) => t,
            Err(_) => return s,
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            match k {
                // Schema marker only; unknown/newer files still load their
                // known keys and fall back to defaults for the rest.
                "version" => {}
                "enabled" => s.enabled = parse_bool(v),
                "mode" => {
                    if matches!(v, "sun" | "custom" | "always") {
                        s.mode = v.to_string()
                    }
                }
                "temp" => s.temp = v.parse().unwrap_or(s.temp).clamp(1000, 6500),
                "strength" => s.strength = v.parse().unwrap_or(s.strength).min(100),
                "fade" => s.fade = v.parse().unwrap_or(s.fade).min(240),
                "lat" => s.lat = v.parse().unwrap_or(s.lat),
                "lon" => s.lon = v.parse().unwrap_or(s.lon),
                "tz" => s.tz = v.parse().unwrap_or(s.tz),
                "wd_start" => s.wd_start = normalize_time(v).unwrap_or(s.wd_start.clone()),
                "wd_end" => s.wd_end = normalize_time(v).unwrap_or(s.wd_end.clone()),
                "weekend" => s.weekend = parse_bool(v),
                "we_start" => s.we_start = normalize_time(v).unwrap_or(s.we_start.clone()),
                "we_end" => s.we_end = normalize_time(v).unwrap_or(s.we_end.clone()),
                "sunset_offset" => s.sunset_offset = v.parse().unwrap_or(0).clamp(-240, 240),
                "sunrise_offset" => s.sunrise_offset = v.parse().unwrap_or(0).clamp(-240, 240),
                "minimize_tray" => s.minimize_tray = parse_bool(v),
                "autostart" => s.autostart = parse_bool(v),
                "dark" => s.dark = parse_bool(v),
                "selected" => s.selected = v.parse().unwrap_or(0),
                "paused_until" => s.paused_until = v.parse().unwrap_or(0),
                "auto_dim" => s.auto_dim = parse_bool(v),
                "dim_day" => s.dim_day = normalize_time(v).unwrap_or(s.dim_day.clone()),
                "dim_evening" => {
                    s.dim_evening = normalize_time(v).unwrap_or(s.dim_evening.clone())
                }
                "dim_night" => s.dim_night = normalize_time(v).unwrap_or(s.dim_night.clone()),
                "dim_day_pct" => {
                    s.dim_day_pct = v.parse().unwrap_or(s.dim_day_pct).clamp(1, 100)
                }
                "dim_evening_pct" => {
                    s.dim_evening_pct = v.parse().unwrap_or(s.dim_evening_pct).clamp(1, 100)
                }
                "dim_night_pct" => {
                    s.dim_night_pct = v.parse().unwrap_or(s.dim_night_pct).clamp(1, 100)
                }
                "dim_fade" => s.dim_fade = v.parse().unwrap_or(s.dim_fade).min(240),
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = fs::create_dir_all(&dir);
        let text = format!(
            "# Luma settings\n\
             version={}\n\
             enabled={}\nmode={}\ntemp={}\nstrength={}\nfade={}\n\
             lat={}\nlon={}\ntz={}\n\
             wd_start={}\nwd_end={}\nweekend={}\nwe_start={}\nwe_end={}\n\
             sunset_offset={}\nsunrise_offset={}\n\
             minimize_tray={}\nautostart={}\ndark={}\nselected={}\npaused_until={}\n\
             auto_dim={}\ndim_day={}\ndim_evening={}\ndim_night={}\n\
             dim_day_pct={}\ndim_evening_pct={}\ndim_night_pct={}\ndim_fade={}\n",
            SETTINGS_SCHEMA,
            self.enabled as u8,
            self.mode,
            self.temp,
            self.strength,
            self.fade,
            self.lat,
            self.lon,
            self.tz,
            self.wd_start,
            self.wd_end,
            self.weekend as u8,
            self.we_start,
            self.we_end,
            self.sunset_offset,
            self.sunrise_offset,
            self.minimize_tray as u8,
            self.autostart as u8,
            self.dark as u8,
            self.selected,
            self.paused_until,
            self.auto_dim as u8,
            self.dim_day,
            self.dim_evening,
            self.dim_night,
            self.dim_day_pct,
            self.dim_evening_pct,
            self.dim_night_pct,
            self.dim_fade,
        );
        // Write beside the real file and rename, so a crash mid-write cannot
        // leave a truncated settings.ini behind.
        let path = config_path();
        let tmp = path.with_extension("ini.tmp");
        if fs::write(&tmp, text).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }
}

pub fn normalize_time(s: &str) -> Option<String> {
    let (h, m) = parse_hhmm(s)?;
    Some(format!("{:02}:{:02}", h, m))
}

pub fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some((h, m))
}

pub fn time_minutes(s: &str, fallback: i32) -> i32 {
    parse_hhmm(s)
        .map(|(h, m)| (h * 60 + m) as i32)
        .unwrap_or(fallback)
}
