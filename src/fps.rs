use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fps {
    pub num: u32,
    pub den: u32,
}

const RATES: [Fps; 7] = [
    Fps::new(24, 1),
    Fps::new(30, 1),
    Fps::new(60, 1),
    Fps::new(120, 1),
    Fps::new(24000, 1001),
    Fps::new(30000, 1001),
    Fps::new(60000, 1001),
];

const DECIMALS: [(&str, Fps); 3] = [
    ("23.976", Fps::new(24000, 1001)),
    ("29.97", Fps::new(30000, 1001)),
    ("59.94", Fps::new(60000, 1001)),
];

const ALL: &str = "24, 30, 60, 120, 24000/1001 (23.976), 30000/1001 (29.97) or 60000/1001 (59.94)";

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

impl Fps {
    pub const fn new(num: u32, den: u32) -> Self {
        Self { num, den }
    }

    pub fn reduced(num: u32, den: u32) -> Option<Self> {
        if num == 0 || den == 0 {
            return None;
        }
        let g = gcd(num, den);
        Some(Self::new(num / g, den / g))
    }

    pub fn parse(s: &str) -> Result<Fps, String> {
        let s = s.trim();
        let known = |f: Option<Fps>| {
            f.filter(|f| RATES.contains(f))
                .ok_or_else(|| ALL.to_string())
        };
        if let Some((n, d)) = s.split_once('/') {
            return known(match (n.trim().parse(), d.trim().parse()) {
                (Ok(n), Ok(d)) => Self::reduced(n, d),
                _ => None,
            });
        }
        if s.contains('.') {
            if let Some((_, f)) = DECIMALS.iter().find(|(d, _)| *d == s) {
                return Ok(*f);
            }
            let v: f64 = s.parse().map_err(|_| ALL.to_string())?;
            let near = RATES
                .iter()
                .min_by(|a, b| (a.rate() - v).abs().total_cmp(&(b.rate() - v).abs()))
                .filter(|f| (f.rate() - v).abs() < 0.1);
            return Err(match near {
                Some(f) => format!("not an exact rate; use {f}"),
                None => ALL.to_string(),
            });
        }
        known(s.parse().ok().and_then(|n| Self::reduced(n, 1)))
    }

    pub fn rate(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    pub fn seconds(self, frame: usize) -> f64 {
        (frame as u64 * self.den as u64) as f64 / self.num as f64
    }

    pub fn frames(self, seconds: f64) -> f64 {
        seconds * self.num as f64 / self.den as f64
    }

    pub fn gop(self) -> u32 {
        (2 * self.num + self.den / 2) / self.den
    }

    pub fn label(self) -> String {
        match DECIMALS.iter().find(|(_, f)| *f == self) {
            Some((d, _)) => d.to_string(),
            None => self.to_string(),
        }
    }
}

impl fmt::Display for Fps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

impl Serialize for Fps {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.den == 1 {
            s.serialize_u32(self.num)
        } else {
            s.serialize_str(&self.to_string())
        }
    }
}

impl<'de> Deserialize<'de> for Fps {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Int(u64),
            Float(f64),
            Text(String),
        }
        let text = match Raw::deserialize(d)? {
            Raw::Int(n) => n.to_string(),
            Raw::Float(v) => v.to_string(),
            Raw::Text(t) => t,
        };
        Fps::parse(&text).map_err(|e| serde::de::Error::custom(format!("fps {text}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILM: Fps = Fps::new(24000, 1001);

    #[test]
    fn the_rates_and_their_decimals_parse_to_exact_rationals() {
        for (s, f) in [
            ("24", Fps::new(24, 1)),
            ("30", Fps::new(30, 1)),
            ("60", Fps::new(60, 1)),
            ("120", Fps::new(120, 1)),
            ("24000/1001", FILM),
            ("30000/1001", Fps::new(30000, 1001)),
            ("60000/1001", Fps::new(60000, 1001)),
            ("23.976", FILM),
            ("29.97", Fps::new(30000, 1001)),
            ("59.94", Fps::new(60000, 1001)),
        ] {
            assert_eq!(Fps::parse(s), Ok(f), "{s}");
        }
        assert_eq!(
            Fps::parse("23.98").unwrap_err(),
            "not an exact rate; use 24000/1001"
        );
        assert_eq!(Fps::parse("30.0").unwrap_err(), "not an exact rate; use 30");
        for s in ["25", "0", "24/0", "48000/2001", "12.5", "x"] {
            assert_eq!(Fps::parse(s).unwrap_err(), ALL, "{s}");
        }
    }

    #[test]
    fn a_film_rate_frame_sits_on_its_rational_time_out_to_a_long_take() {
        for k in [1usize, 2, 36, 360, 1000] {
            assert_eq!(FILM.seconds(24000 * k), 1001.0 * k as f64);
        }
        for n in [
            0usize, 1, 23, 24, 1001, 86_313, 863_136, 2_589_408, 10_000_000,
        ] {
            let t = FILM.seconds(n);
            assert_eq!(t, (n as u64 * 1001) as f64 / 24000.0);
            assert_eq!(FILM.frames(t).round() as usize, n);
            assert!(n == 0 || FILM.seconds(n - 1) < t);
        }
        assert!(FILM.seconds(863_136) - 863_136.0 / 24.0 > 35.0);
    }

    #[test]
    fn a_rate_round_trips_through_json_and_toml() {
        assert_eq!(
            serde_json::to_value(FILM).unwrap(),
            serde_json::json!("24000/1001")
        );
        assert_eq!(
            serde_json::to_value(Fps::new(30, 1)).unwrap(),
            serde_json::json!(30)
        );
        assert_eq!(
            serde_json::from_value::<Fps>(serde_json::json!("24000/1001")).unwrap(),
            FILM
        );
        assert_eq!(
            serde_json::from_value::<Fps>(serde_json::json!(60)).unwrap(),
            Fps::new(60, 1)
        );
        #[derive(Deserialize)]
        struct S {
            fps: Fps,
        }
        assert_eq!(toml::from_str::<S>("fps = 23.976").unwrap().fps, FILM);
        assert_eq!(
            toml::from_str::<S>("fps = \"29.97\"").unwrap().fps,
            Fps::new(30000, 1001)
        );
        assert_eq!(
            toml::from_str::<S>("fps = 24").unwrap().fps,
            Fps::new(24, 1)
        );
        assert!(toml::from_str::<S>("fps = 25").is_err());
    }

    #[test]
    fn a_film_rate_keeps_a_two_second_keyframe_and_its_stem_label() {
        assert_eq!(FILM.gop(), 48);
        assert_eq!(Fps::new(30000, 1001).gop(), 60);
        assert_eq!(Fps::new(30, 1).gop(), 60);
        assert_eq!(FILM.label(), "23.976");
        assert_eq!(Fps::new(120, 1).label(), "120");
    }
}
