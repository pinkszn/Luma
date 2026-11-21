//! Time-of-day brightness curve.
//!
//! The curve is anchored at `day` (the moment the day starts). Brightness is
//! `day_pct` during the day, ramps down to `evening_pct` at `evening`, holds
//! until `night`, ramps to `night_pct`, holds overnight, then ramps back to
//! `day_pct` at `day` over `fade` minutes. Every ramp is linear, so the display
//! changes by a fraction of a percent per tick instead of jumping.

#[derive(Clone, Copy, Debug)]
pub struct Curve {
    pub day: u16,
    pub evening: u16,
    pub night: u16,
    pub day_pct: u8,
    pub evening_pct: u8,
    pub night_pct: u8,
    pub fade: u16,
}

impl Default for Curve {
    fn default() -> Self {
        Self {
            day: 7 * 60,
            evening: 21 * 60,
            night: 23 * 60,
            day_pct: 100,
            evening_pct: 75,
            night_pct: 35,
            fade: 20,
        }
    }
}

pub const PHASE_DAY: &str = "day";
pub const PHASE_EVENING: &str = "evening";
pub const PHASE_NIGHT: &str = "night";
pub const PHASE_MORNING: &str = "morning";

/// True when the three anchors are ordered and the percentages are sane.
/// Without ordering there is no meaningful day/evening/night split.
pub fn valid(c: &Curve) -> bool {
    c.day < c.evening && c.evening < c.night && c.day_pct >= c.evening_pct && c.evening_pct >= c.night_pct
}

/// Brightness target in percent for a local time expressed in minutes.
pub fn target(c: &Curve, minute: u16) -> u8 {
    let (pct, _) = target_phase(c, minute);
    pct
}

/// Brightness target plus the name of the phase it belongs to.
pub fn target_phase(c: &Curve, minute: u16) -> (u8, &'static str) {
    let day = c.day % 1440;
    let evening = (c.evening + 1440 - day) % 1440;
    let night = (c.night + 1440 - day) % 1440;
    let m = (minute % 1440 + 1440 - day) % 1440;
    let fade = c.fade.max(1).min(720) as u32;
    let day_pct = c.day_pct.min(100) as f32;
    let evening_pct = c.evening_pct.min(c.day_pct).min(100) as f32;
    let night_pct = c.night_pct.min(c.evening_pct).min(100) as f32;

    if m < evening {
        if (m as u32) < fade {
            let t = m as f32 / fade as f32;
            return (lerp(night_pct, day_pct, t), PHASE_MORNING);
        }
        (c.day_pct.min(100), PHASE_DAY)
    } else if m < night {
        let t = ((m - evening) as f32 / fade as f32).min(1.0);
        (lerp(day_pct, evening_pct, t), PHASE_EVENING)
    } else {
        let t = ((m - night) as f32 / fade as f32).min(1.0);
        (lerp(evening_pct, night_pct, t), PHASE_NIGHT)
    }
}

/// Minutes until the next anchor, for the status line.
pub fn next_change(c: &Curve, minute: u16) -> (u16, &'static str) {
    let now = minute % 1440;
    let anchors = [
        (c.evening % 1440, "evening"),
        (c.night % 1440, "night"),
        (c.day % 1440, "day"),
    ];
    let mut best = (1440u16, "day");
    for (min, label) in anchors {
        let mut d = (min + 1440 - now) % 1440;
        if d == 0 {
            d = 1440;
        }
        if d < best.0 {
            best = (d, label);
        }
    }
    best
}

fn lerp(a: f32, b: f32, t: f32) -> u8 {
    (a + (b - a) * t.clamp(0.0, 1.0)).round().clamp(0.0, 100.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> Curve {
        Curve::default()
    }

    #[test]
    fn midday_is_full() {
        assert_eq!(target(&curve(), 12 * 60), 100);
        assert_eq!(target(&curve(), 20 * 60), 100);
    }

    #[test]
    fn evening_ramp_lands_on_evening_pct() {
        let c = curve();
        assert_eq!(target(&c, 21 * 60), 100);
        assert_eq!(target(&c, 21 * 60 + 10), 88);
        assert_eq!(target(&c, 22 * 60), 75);
    }

    #[test]
    fn night_holds_until_morning() {
        let c = curve();
        assert_eq!(target(&c, 23 * 60), 75);
        assert_eq!(target(&c, 23 * 60 + 10), 55);
        assert_eq!(target(&c, 2 * 60), 35);
        assert_eq!(target(&c, 6 * 60), 35);
    }

    #[test]
    fn morning_ramp_returns_to_full() {
        let c = curve();
        assert_eq!(target(&c, 7 * 60), 35);
        assert_eq!(target(&c, 7 * 60 + 10), 68);
        assert_eq!(target(&c, 8 * 60), 100);
    }

    #[test]
    fn wrap_around_midnight_is_stable() {
        let c = curve();
        assert_eq!(target(&c, 0), target(&c, 24 * 60));
    }

    #[test]
    fn day_level_is_configurable() {
        let mut c = curve();
        c.day_pct = 80;
        c.evening_pct = 60;
        c.night_pct = 30;
        assert!(valid(&c));
        assert_eq!(target(&c, 12 * 60), 80);
        assert_eq!(target(&c, 20 * 60), 80);
        assert_eq!(target(&c, 22 * 60), 60);
        assert_eq!(target(&c, 2 * 60), 30);
        assert_eq!(target(&c, 8 * 60), 80);
    }

    #[test]
    fn day_level_below_evening_is_invalid() {
        let mut c = curve();
        c.day_pct = 50;
        assert!(!valid(&c));
    }

    #[test]
    fn rejects_unordered_anchors() {
        let mut c = curve();
        c.night = 20 * 60;
        assert!(!valid(&c));
    }

    #[test]
    fn next_change_reports_upcoming_anchor() {
        let c = curve();
        assert_eq!(next_change(&c, 12 * 60), (9 * 60, "evening"));
        assert_eq!(next_change(&c, 22 * 60), (60, "night"));
        assert_eq!(next_change(&c, 23 * 60), (8 * 60, "day"));
    }

    #[test]
    fn full_day_target_is_smooth() {
        let c = curve();
        assert!(valid(&c));
        let mut prev = target(&c, 0);
        assert!(prev <= 100);
        for m in 1..=24 * 60 {
            let t = target(&c, m);
            assert!(t <= 100, "target over 100 at {m}");
            assert!(
                t.abs_diff(prev) <= 10,
                "jump at minute {m}: {prev} -> {t}"
            );
            prev = t;
        }
    }
}
