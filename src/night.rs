//! Night-light schedule math: solar times, weekday/weekend windows, fades,
//! and the gamma ramp that implements the tint.

use crate::settings::{time_minutes, Settings};
use crate::win::{local_now, SYSTEMTIME};

#[derive(Clone, Copy)]
pub struct LocalNow {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub weekday: u32,
    pub minutes: i32,
}

pub fn now_info() -> LocalNow {
    let st: SYSTEMTIME = unsafe { local_now() };
    LocalNow {
        year: st.wYear as i32,
        month: st.wMonth as u32,
        day: st.wDay as u32,
        weekday: st.wDayOfWeek as u32,
        minutes: st.wHour as i32 * 60 + st.wMinute as i32,
    }
}

pub struct NightInfo {
    pub active: bool,
    pub blend: f32,
    pub kelvin: u32,
    pub status: String,
    pub start_min: i32,
    pub end_min: i32,
    pub solar_ok: bool,
}

fn day_of_year(y: i32, m: u32, d: u32) -> u32 {
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let mut doy = CUM[(m - 1) as usize] + d;
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    if leap && m > 2 {
        doy += 1;
    }
    doy
}

/// Returns (sunrise, sunset) in local minutes for the given date.
pub fn solar_times(lat: f64, lon: f64, tz: f64, year: i32, month: u32, day: u32) -> Option<(f64, f64)> {
    let doy = day_of_year(year, month, day) as f64;
    let decl = 23.44_f64.to_radians() * (2.0 * std::f64::consts::PI * (284.0 + doy) / 365.0).sin();
    let b = (2.0 * std::f64::consts::PI * (doy - 81.0) / 365.0).to_radians();
    let eot = 9.87 * (2.0 * b).sin() - 7.53 * b.cos() - 1.5 * b.sin(); // minutes
    let solar_noon = 720.0 - 4.0 * lon - eot + tz * 60.0;
    let lat_r = lat.to_radians();
    let cos_ha = (90.833_f64.to_radians().cos() / (lat_r.cos() * decl.cos()))
        - lat_r.tan() * decl.tan();
    if !cos_ha.is_finite() || cos_ha > 1.0 || cos_ha < -1.0 {
        return None;
    }
    let ha = cos_ha.acos().to_degrees();
    let sunrise = solar_noon - 4.0 * ha;
    let sunset = solar_noon + 4.0 * ha;
    Some((sunrise.rem_euclid(1440.0), sunset.rem_euclid(1440.0)))
}

pub fn fmt_min(m: i32) -> String {
    let m = m.rem_euclid(1440);
    format!("{:02}:{:02}", m / 60, m % 60)
}

fn fmt_duration(mins: i32) -> String {
    let mins = mins.max(0);
    let h = mins / 60;
    let m = mins % 60;
    if h > 0 {
        format!("{} h {:02} min", h, m)
    } else {
        format!("{} min", m)
    }
}

fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

pub fn solar_summary(s: &Settings, now: LocalNow) -> String {
    match solar_times(s.lat, s.lon, s.tz, now.year, now.month, now.day) {
        Some((rise, set)) => format!(
            "Sunrise {} · Sunset {}\n{:.4}, {:.4} ·  UTC{}{}",
            fmt_min(rise as i32 + s.sunrise_offset),
            fmt_min(set as i32 + s.sunset_offset),
            s.lat,
            s.lon,
            if s.tz >= 0.0 { "+" } else { "-" },
            fmt_offset(s.tz)
        ),        None => "No sunrise or sunset at this latitude today (polar day or night).".to_string(),
    }
}

fn fmt_offset(tz: f64) -> String {
    let a = tz.abs();
    let h = a.floor() as i32;
    let m = ((a - h as f64) * 60.0).round() as i32;
    format!("{:02}:{:02}", h, m)
}

/// Compute the active window for the current date. Returns
/// (start_min, end_min, active, elapsed, duration, description).
fn window(
    s: &Settings,
    now: LocalNow,
) -> (i32, i32, bool, i32, i32, String, bool) {
    let evening = s.mode == "sun";
    if evening {
        if let Some((rise, set)) = solar_times(s.lat, s.lon, s.tz, now.year, now.month, now.day) {
            let start = (set as i32 + s.sunset_offset).rem_euclid(1440);
            let end = (rise as i32 + s.sunrise_offset).rem_euclid(1440);
            let (active, elapsed, dur) = evaluate(now.minutes, start, end);
            let desc = format!("sunset {} → sunrise {}", fmt_min(start), fmt_min(end));
            return (start, end, active, elapsed, dur, desc, true);
        }
        // Polar fallback: 19:00–07:00.
        let start = (19 * 60 + s.sunset_offset).rem_euclid(1440);
        let end = (7 * 60 + s.sunrise_offset).rem_euclid(1440);
        let (active, elapsed, dur) = evaluate(now.minutes, start, end);
        return (start, end, active, elapsed, dur, "polar fallback 19:00 → 07:00".into(), false);
    }

    let weekend_day = s.weekend && (now.weekday == 0 || now.weekday == 6);
    let (start_s, end_s) = if weekend_day {
        (&s.we_start, &s.we_end)
    } else {
        (&s.wd_start, &s.wd_end)
    };
    let start = time_minutes(start_s, 19 * 60);
    let end = time_minutes(end_s, 7 * 60);
    let (active, elapsed, dur) = evaluate(now.minutes, start, end);
    let desc = format!(
        "{} → {}{}",
        fmt_min(start),
        fmt_min(end),
        if weekend_day { " (weekend)" } else { "" }
    );
    (start, end, active, elapsed, dur, desc, true)
}

fn evaluate(now: i32, start: i32, end: i32) -> (bool, i32, i32) {
    if start == end {
        return (true, 0, 1440);
    }
    if start < end {
        if now >= start && now < end {
            let dur = end - start;
            (true, now - start, dur)
        } else {
            (false, 0, end - start)
        }
    } else {
        let dur = (end - start).rem_euclid(1440);
        if now >= start {
            (true, now - start, dur)
        } else if now < end {
            (true, now + 1440 - start, dur)
        } else {
            (false, 0, dur)
        }
    }
}

pub fn compute(s: &Settings, now: LocalNow, now_epoch: u64) -> NightInfo {
    if !s.enabled {
        return NightInfo {
            active: false,
            blend: 0.0,
            kelvin: s.temp,
            status: "Night light is off.".into(),
            start_min: 0,
            end_min: 0,
            solar_ok: true,
        };
    }
    if s.paused_until > now_epoch {
        let left = ((s.paused_until - now_epoch) as i32 + 59) / 60;
        return NightInfo {
            active: false,
            blend: 0.0,
            kelvin: s.temp,
            status: format!("Paused · resumes in {} min.", left),
            start_min: 0,
            end_min: 0,
            solar_ok: true,
        };
    }

    if s.mode == "always" {
        return NightInfo {
            active: true,
            blend: s.strength as f32 / 100.0,
            kelvin: s.temp,
            status: format!("Always on · {} K · {}% strength.", s.temp, s.strength),
            start_min: 0,
            end_min: 1440,
            solar_ok: true,
        };
    }

    let (start, end, active, elapsed, dur, desc, solar_ok) = window(s, now);
    let fade = s.fade.max(0) as i32;
    let mut ramp = 1.0f64;
    if active {
        if fade > 0 {
            let falling = smooth(elapsed as f64 / fade as f64);
            let rising = smooth((dur - elapsed) as f64 / fade as f64);
            ramp = falling.min(rising);
        }
    } else {
        ramp = 0.0;
    }
    let blend = (s.strength as f64 / 100.0 * ramp).clamp(0.0, 1.0) as f32;

    let status = if active {
        let left = (dur - elapsed).max(1);
        format!(
            "On · {} K at {}% · ends {} (in {})",
            s.temp,
            (blend * 100.0).round() as u32,
            fmt_min(end),
            fmt_duration(left)
        )
    } else {
        let delta = (start - now.minutes).rem_euclid(1440).max(1);
        format!(
            "Off · starts {} (in {}) · {}",
            fmt_min(start),
            fmt_duration(delta),
            desc
        )
    };

    NightInfo {
        active,
        blend,
        kelvin: s.temp,
        status,
        start_min: start,
        end_min: end,
        solar_ok,
    }
}

pub fn build_ramp(kelvin: u32, blend: f32) -> [u16; 768] {
    let (r, g, b) = kelvin_rgb(kelvin);
    let blend = blend.clamp(0.0, 1.0) as f64;
    let mut ramp = [0u16; 768];
    for i in 0..256usize {
        let v = i as f64 / 255.0;
        let rr = (v * (1.0 + (r - 1.0) * blend)).clamp(0.0, 1.0);
        let gg = (v * (1.0 + (g - 1.0) * blend)).clamp(0.0, 1.0);
        let bb = (v * (1.0 + (b - 1.0) * blend)).clamp(0.0, 1.0);
        ramp[i] = (rr * 65535.0).round() as u16;
        ramp[256 + i] = (gg * 65535.0).round() as u16;
        ramp[512 + i] = (bb * 65535.0).round() as u16;
    }
    ramp
}

pub fn identity_ramp() -> [u16; 768] {
    let mut ramp = [0u16; 768];
    for i in 0..256usize {
        let v = (i as f64 / 255.0 * 65535.0).round() as u16;
        ramp[i] = v;
        ramp[256 + i] = v;
        ramp[512 + i] = v;
    }
    ramp
}

/// Black-body-ish RGB, normalized so the brightest channel is 1.0.
fn kelvin_rgb(k: u32) -> (f64, f64, f64) {
    let t = (k.clamp(1000, 6500) as f64) / 100.0;
    let (r, g, b) = if t <= 66.0 {
        let r = 255.0;
        let g = 99.4708025861 * t.ln() - 161.1195681661;
        let b = if t <= 19.0 {
            0.0
        } else {
            138.5177312231 * (t - 10.0).ln() - 305.0447927307
        };
        (r, g, b)
    } else {
        let r = 329.698727446 * (t - 60.0).powf(-0.1332047592);
        let g = 288.1221695283 * (t - 60.0).powf(-0.0755148492);
        (r, g, 255.0)
    };
    let max = r.max(g).max(b).max(1.0);
    (
        (r / max).clamp(0.0, 1.0),
        (g / max).clamp(0.0, 1.0),
        (b / max).clamp(0.0, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manila_solar_sane() {
        let (rise, set) = solar_times(14.676, 121.0437, 8.0, 2026, 11, 21).unwrap();
        assert!(rise > 5.0 * 60.0 && rise < 7.0 * 60.0, "sunrise {rise}");
        assert!(set > 17.0 * 60.0 && set < 18.5 * 60.0, "sunset {set}");
    }

    #[test]
    fn overnight_window() {
        let (a, e, d) = evaluate(23 * 60, 19 * 60, 7 * 60);
        assert!(a);
        assert_eq!(e, 240);
        assert_eq!(d, 720);
        let (a, _, _) = evaluate(6 * 60, 19 * 60, 7 * 60);
        assert!(a);
        let (a, _, _) = evaluate(12 * 60, 19 * 60, 7 * 60);
        assert!(!a);
    }

    #[test]
    fn ramp_identity_at_zero_blend() {
        let r = build_ramp(2000, 0.0);
        assert_eq!(r[0], 0);
        assert_eq!(r[255], 65535);
        assert_eq!(r[768 - 1], 65535);
    }

    fn custom_settings() -> Settings {
        let mut s = Settings::default();
        s.enabled = true;
        s.mode = "custom".into();
        s.wd_start = "19:00".into();
        s.wd_end = "07:00".into();
        s.weekend = true;
        s.we_start = "20:00".into();
        s.we_end = "08:00".into();
        s.fade = 30;
        s.strength = 55;
        s.temp = 3400;
        s
    }

    fn at(weekday: u32, minutes: i32) -> LocalNow {
        LocalNow {
            year: 2026,
            month: 11,
            day: 21,
            weekday,
            minutes,
        }
    }

    #[test]
    fn full_day_schedule_is_continuous() {
        let s = custom_settings();
        let mut active_minutes = 0;
        let mut prev: Option<f32> = None;
        let mut peak = 0.0f32;
        for m in 0..1440 {
            let info = compute(&s, at(1, m), 0); // Monday
            if info.active {
                active_minutes += 1;
            }
            peak = peak.max(info.blend);
            if let Some(p) = prev {
                assert!(
                    (info.blend - p).abs() <= (0.55 / 30.0) + 0.02,
                    "discontinuity at {m}: {p} -> {}",
                    info.blend
                );
            }
            prev = Some(info.blend);
        }
        assert_eq!(active_minutes, 720, "overnight window should be 12 h");
        assert!((peak - 0.55).abs() < 0.02, "peak blend {peak}");
        assert!(!compute(&s, at(1, 12 * 60), 0).active);
        assert!(compute(&s, at(1, 23 * 60), 0).active);
    }

    #[test]
    fn weekend_window_is_used_on_saturday() {
        let s = custom_settings();
        let fri = compute(&s, at(5, 19 * 60 + 30), 0);
        let sat = compute(&s, at(6, 19 * 60 + 30), 0);
        assert!(fri.active, "Friday uses weekday 19:00 window");
        assert!(!sat.active, "Saturday starts at 20:00");
        assert!(compute(&s, at(6, 20 * 60 + 1), 0).active);
    }

    #[test]
    fn pause_suppresses_schedule() {
        let mut s = custom_settings();
        let now = 1_000_000u64;
        s.paused_until = now + 30 * 60;
        let info = compute(&s, at(1, 23 * 60), now);
        assert!(!info.active);
        assert_eq!(info.blend, 0.0);
        assert!(info.status.contains("Paused"));
    }
}
