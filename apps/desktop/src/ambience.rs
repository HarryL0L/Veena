//! The living background on Home: what the sky looks like now, the season,
//! special days, and the words and suggestions that go with them. The same
//! rules as the Android app.

use std::path::PathBuf;

use jiff::ZonedDateTime;
use jiff::civil::Date;
use slint::Color;

use crate::SceneInfo;

/// The user's choices for the living background, kept in a small text file.
#[derive(Clone, Debug, PartialEq)]
pub struct AmbienceSettings {
    pub enabled: bool,
    /// None follows the clock; 0 dawn, 1 day, 2 dusk, 3 night.
    pub day_part: Option<i32>,
    /// None follows the date; 0 spring, 1 summer, 2 autumn, 3 winter.
    pub season: Option<i32>,
    pub southern: bool,
    pub special_days: bool,
}

impl Default for AmbienceSettings {
    fn default() -> Self {
        AmbienceSettings {
            enabled: true,
            day_part: None,
            season: None,
            southern: guess_southern(),
            special_days: true,
        }
    }
}

fn settings_file() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("veena").join("desktop.conf")
}

impl AmbienceSettings {
    pub fn load() -> Self {
        let mut s = AmbienceSettings::default();
        let Ok(text) = std::fs::read_to_string(settings_file()) else {
            return s;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let num = || v.parse::<i32>().ok().filter(|n| (0..4).contains(n));
            match k {
                "ambience" => s.enabled = v == "true",
                "day_part" => s.day_part = num(),
                "season" => s.season = num(),
                "southern" => s.southern = v == "true",
                "special_days" => s.special_days = v == "true",
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let file = settings_file();
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let opt = |o: Option<i32>| o.map(|n| n.to_string()).unwrap_or_else(|| "auto".into());
        let text = format!(
            "ambience={}\nday_part={}\nseason={}\nsouthern={}\nspecial_days={}\n",
            self.enabled,
            opt(self.day_part),
            opt(self.season),
            self.southern,
            self.special_days
        );
        if let Err(e) = std::fs::write(&file, text) {
            eprintln!("veena: could not save settings: {e}");
        }
    }
}

/// A first guess from the time zone; changeable in Settings.
fn guess_southern() -> bool {
    let zone = std::env::var("TZ")
        .ok()
        .or_else(|| {
            std::fs::read_link("/etc/localtime")
                .ok()
                .map(|p| p.display().to_string())
        })
        .unwrap_or_default();
    const SOUTH: &[&str] = &[
        "Australia/",
        "Antarctica/",
        "Pacific/Auckland",
        "Pacific/Chatham",
        "Pacific/Fiji",
        "America/Argentina",
        "America/Buenos_Aires",
        "America/Santiago",
        "America/Sao_Paulo",
        "America/Montevideo",
        "America/Asuncion",
        "America/La_Paz",
        "America/Lima",
        "Africa/Johannesburg",
        "Africa/Maputo",
        "Africa/Harare",
        "Africa/Windhoek",
        "Africa/Gaborone",
        "Africa/Lusaka",
        "Indian/Mauritius",
        "Indian/Reunion",
        "Indian/Antananarivo",
    ];
    SOUTH.iter().any(|s| zone.contains(s))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Occasion {
    Diwali = 1,
    NewYear = 2,
    Valentine = 3,
}

/// Everything Home needs to show the moment.
pub struct Moment {
    pub hour: f32,
    pub season: i32,
    pub occasion: Option<Occasion>,
    pub date: Date,
}

pub const SEASONS: [&str; 4] = ["Spring", "Summer", "Autumn", "Winter"];
pub const DAY_PARTS: [&str; 4] = ["Dawn", "Day", "Dusk", "Night"];

impl Moment {
    pub fn now(s: &AmbienceSettings) -> Moment {
        let now = ZonedDateTime::now();
        let date = now.date();
        let hour = match s.day_part {
            Some(0) => 6.6,
            Some(1) => 12.0,
            Some(2) => 18.4,
            Some(_) => 23.0,
            None => now.hour() as f32 + now.minute() as f32 / 60.0,
        };
        Moment {
            hour,
            season: s.season.unwrap_or_else(|| season_of(date, s.southern)),
            occasion: if s.special_days {
                occasion_of(date)
            } else {
                None
            },
            date,
        }
    }

    pub fn day_part(&self) -> usize {
        let h = self.hour;
        if (5.0..8.0).contains(&h) {
            0
        } else if (8.0..17.0).contains(&h) {
            1
        } else if (17.0..20.0).contains(&h) {
            2
        } else {
            3
        }
    }

    pub fn greeting(&self) -> &'static str {
        match self.occasion {
            Some(Occasion::Diwali) => "Happy Diwali",
            Some(Occasion::NewYear) => "Happy New Year",
            Some(Occasion::Valentine) => "Happy Valentine's Day",
            None => match self.hour {
                h if (5.0..12.0).contains(&h) => "Good morning",
                h if (12.0..17.0).contains(&h) => "Good afternoon",
                h if (17.0..22.0).contains(&h) => "Good evening",
                _ => "Hello, night owl",
            },
        }
    }

    pub fn line(&self) -> &'static str {
        match self.occasion {
            Some(Occasion::Diwali) => return "Lamps lit, sweets shared, music on.",
            Some(Occasion::NewYear) => return "A fresh year of songs ahead.",
            Some(Occasion::Valentine) => return "Play something for someone.",
            None => {}
        }
        match (self.day_part(), self.season) {
            (0, 3) => "A frosty sunrise. Ease into the day.",
            (0, _) => "The sun's coming up. Ease into the day.",
            (1, 0) => "Blossoms out. Something bright?",
            (1, 1) => "Long sunny day. Turn it up.",
            (1, 2) => "Leaves falling. Something warm?",
            (1, _) => "Snow outside, songs inside.",
            (2, _) => "The sky's turning gold. Time to unwind.",
            (_, 1) => "Warm night, fireflies out.",
            _ => "Quiet night, soft stars.",
        }
    }

    pub fn date_line(&self) -> String {
        format!(
            "{} · {}",
            self.date.strftime("%A, %-d %B"),
            SEASONS[self.season as usize]
        )
    }

    pub fn moment_name(&self) -> &'static str {
        match self.day_part() {
            0 => "morning",
            1 if self.hour < 12.0 => "morning",
            1 => "afternoon",
            2 => "evening",
            _ => "night",
        }
    }

    /// Searches that suit the moment.
    pub fn moods(&self) -> Vec<&'static str> {
        let time: [&str; 4] = match self.hour {
            h if (5.0..12.0).contains(&h) => [
                "Morning acoustic",
                "Coffee jazz",
                "Upbeat pop",
                "Morning ragas",
            ],
            h if (12.0..17.0).contains(&h) => ["Focus beats", "Feel-good hits", "Indie", "Workout"],
            h if (17.0..22.0).contains(&h) => [
                "Sunset chill",
                "Evening jazz",
                "Retro classics",
                "Road trip",
            ],
            _ => ["Late night lo-fi", "Soft piano", "Ambient", "Sleep"],
        };
        let mut list: Vec<&str> = match self.occasion {
            Some(Occasion::Diwali) => vec!["Diwali songs", "Festive dhol"],
            Some(Occasion::NewYear) => vec!["Party anthems", "Countdown hits"],
            Some(Occasion::Valentine) => vec!["Love songs", "Romantic duets"],
            None => vec![
                [
                    "Spring vibes",
                    "Summer hits",
                    "Autumn acoustic",
                    "Cozy winter",
                ][self.season as usize],
            ],
        };
        list.extend(time);
        list
    }

    /// The sky's colours and where the sun and moon are, blended into `page`.
    pub fn scene(&self, page: Color) -> SceneInfo {
        let h = self.hour;
        let (top, bottom) = sky_at(h);
        let tone = |c: Color| mix(c, page, 0.22);
        let night = nightness(h);
        let gold = goldenness(h);

        let (mut sun_x, mut sun_y, mut sun_alpha) = (0.0, 0.0, 0.0);
        if (5.4..=19.6).contains(&h) {
            let f = ((h - 5.6) / 13.8).clamp(0.0, 1.0);
            (sun_x, sun_y) = arc(f);
            sun_alpha = ((h - 5.4).min(19.6 - h) / 0.8).clamp(0.0, 1.0);
        }
        let mh = if h < 12.0 { h + 24.0 } else { h };
        let (mut moon_x, mut moon_y, mut moon_alpha) = (0.0, 0.0, 0.0);
        if (18.8..=30.2).contains(&mh) {
            let f = ((mh - 19.0) / 11.0).clamp(0.0, 1.0);
            (moon_x, moon_y) = arc(f);
            moon_alpha = ((mh - 18.8).min(30.2 - mh) / 0.8).clamp(0.0, 1.0);
        }
        SceneInfo {
            sky_top: tone(top),
            sky_bottom: tone(bottom),
            night,
            gold,
            sun_x,
            sun_y,
            sun_alpha,
            sun_color: mix(rgb(0xFFF1C1), rgb(0xFFA36C), gold),
            moon_x,
            moon_y,
            moon_alpha,
            season: self.season,
            occasion: self.occasion.map(|o| o as i32).unwrap_or(0),
        }
    }
}

/// Whole months, flipped for the southern hemisphere.
pub fn season_of(date: Date, southern: bool) -> i32 {
    let north = match date.month() {
        3..=5 => 0,
        6..=8 => 1,
        9..=11 => 2,
        _ => 3,
    };
    if southern { (north + 2) % 4 } else { north }
}

/// Lakshmi Puja dates (India); the lights stay up two days either side.
const DIWALI: [(i16, i8, i8); 5] = [
    (2026, 11, 8),
    (2027, 10, 29),
    (2028, 10, 17),
    (2029, 11, 5),
    (2030, 10, 26),
];

pub fn occasion_of(date: Date) -> Option<Occasion> {
    let diwali = DIWALI
        .iter()
        .any(|&(y, m, d)| Date::new(y, m, d).is_ok_and(|day| (date - day).get_days().abs() <= 2));
    if diwali {
        Some(Occasion::Diwali)
    } else if (date.month() == 12 && date.day() == 31) || (date.month() == 1 && date.day() == 1) {
        Some(Occasion::NewYear)
    } else if date.month() == 2 && date.day() == 14 {
        Some(Occasion::Valentine)
    } else {
        None
    }
}

fn rgb(hex: u32) -> Color {
    Color::from_rgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// `a` moved towards `b` by `f`.
fn mix(a: Color, b: Color, f: f32) -> Color {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * f).round() as u8;
    Color::from_rgb_u8(
        l(a.red(), b.red()),
        l(a.green(), b.green()),
        l(a.blue(), b.blue()),
    )
}

const SKY: [(f32, u32, u32); 9] = [
    (0.0, 0x0B1026, 0x1B2A4A),
    (4.8, 0x141A38, 0x2E2C55),
    (6.4, 0x3E4A89, 0xF2A07B),
    (8.5, 0x5B9BD5, 0xBFE0F5),
    (12.0, 0x4A90D9, 0xA9D4F5),
    (16.3, 0x5A97D0, 0xCDE3F2),
    (18.4, 0x3A3F7A, 0xF08A5D),
    (20.0, 0x1C2347, 0x4A3B66),
    (24.0, 0x0B1026, 0x1B2A4A),
];

fn sky_at(hour: f32) -> (Color, Color) {
    let h = hour.rem_euclid(24.0);
    let i = SKY
        .iter()
        .rposition(|k| k.0 <= h)
        .unwrap_or(0)
        .min(SKY.len() - 2);
    let (a, b) = (SKY[i], SKY[i + 1]);
    let f = ((h - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    (mix(rgb(a.1), rgb(b.1), f), mix(rgb(a.2), rgb(b.2), f))
}

fn nightness(h: f32) -> f32 {
    if !(4.5..20.5).contains(&h) {
        1.0
    } else if h < 7.5 {
        1.0 - (h - 4.5) / 3.0
    } else if h < 17.5 {
        0.0
    } else {
        (h - 17.5) / 3.0
    }
}

fn goldenness(h: f32) -> f32 {
    let peak = |c: f32| (1.0 - (h - c).abs() / 1.6).clamp(0.0, 1.0);
    peak(6.6).max(peak(18.4))
}

/// A point on the arc across the sky, as fractions of the scene's size.
fn arc(f: f32) -> (f32, f32) {
    (0.1 + 0.8 * f, 0.7 - 0.55 * (std::f32::consts::PI * f).sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seasons_flip_in_the_south() {
        let june = Date::new(2027, 6, 15).unwrap();
        assert_eq!(season_of(june, false), 1);
        assert_eq!(season_of(june, true), 3);
    }

    #[test]
    fn special_days() {
        let d = |m, day| Date::new(2027, m, day).unwrap();
        assert_eq!(occasion_of(d(10, 30)), Some(Occasion::Diwali));
        assert_eq!(occasion_of(d(1, 1)), Some(Occasion::NewYear));
        assert_eq!(occasion_of(d(2, 14)), Some(Occasion::Valentine));
        assert_eq!(occasion_of(d(3, 3)), None);
    }

    #[test]
    fn sun_by_day_moon_by_night() {
        let at = |hour| Moment {
            hour,
            season: 0,
            occasion: None,
            date: Date::new(2027, 3, 3).unwrap(),
        };
        let noon = at(12.0).scene(Color::from_rgb_u8(0, 0, 0));
        assert!(noon.sun_alpha > 0.9 && noon.moon_alpha == 0.0);
        let midnight = at(0.5).scene(Color::from_rgb_u8(0, 0, 0));
        assert!(midnight.sun_alpha == 0.0 && midnight.moon_alpha > 0.9);
    }
}
