//! What the weather looks like (decision T565): the client's `Weather`
//! table, read once: each kind's rows by their intensity, with their
//! color. A world profile's weather is a kind's plain rows (those naming
//! no texture of their own; the others are zones' own looks, as red rain).

use crate::Casc;
use crate::tables::Table;

/// The kinds a world has (`Enum.WeatherType`): clear, rain, snow and
/// sandstorm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Clear = 0,
    Rain = 1,
    Snow = 2,
    Sandstorm = 3,
}

impl Kind {
    /// A kind by its name (`"clear"`, `"rain"`, `"snow"`, `"sandstorm"`).
    pub fn named(name: &str) -> Option<Kind> {
        match name {
            "clear" => Some(Kind::Clear),
            "rain" => Some(Kind::Rain),
            "snow" => Some(Kind::Snow),
            "sandstorm" => Some(Kind::Sandstorm),
            _ => None,
        }
    }

    /// A kind by its number (`Enum.WeatherType`); others none.
    pub fn of(n: u32) -> Option<Kind> {
        [Kind::Clear, Kind::Rain, Kind::Snow, Kind::Sandstorm]
            .into_iter()
            .find(|k| *k as u32 == n)
    }

    /// What falls where a row names no texture (the files the Classic
    /// clients' rows name for each kind; the game's own unverified): a
    /// rain streak, a snowflake, sand.
    pub fn texture(self) -> Option<u32> {
        match self {
            Kind::Clear => None,
            Kind::Rain => Some(186_226),
            Kind::Snow => Some(186_228),
            Kind::Sandstorm => Some(186_229),
        }
    }
}

/// How a weather looks: what falls and its color.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub texture: u32,
    pub color: [f32; 3],
}

#[derive(Clone, Debug)]
struct Row {
    kind: u32,
    intensity: f32,
    texture: u32,
    color: [f32; 3],
}

/// The build's weathers.
#[derive(Debug, Default)]
pub struct Weathers {
    rows: Vec<Row>,
}

impl Weathers {
    /// Reads the build's table; none where it lacks it.
    pub fn read(casc: &Casc) -> Option<Weathers> {
        let t = Table::open(casc, "Weather")?;
        let rows = t
            .rows()
            .map(|r| {
                let i = r.floats("Intensity");
                let c = r.floats("EffectColor");
                Row {
                    // (a byte: the layouts' junk above masked)
                    kind: r.int("Type") & 0xFF,
                    intensity: i.first().copied().unwrap_or(0.0),
                    texture: r.int("EffectTextureFileDataID"),
                    color: [0, 1, 2].map(|k| c.get(k).copied().unwrap_or(1.0)),
                }
            })
            .collect();
        Some(Weathers { rows })
    }

    /// How a kind of weather looks at an intensity (0 to 1): the kind's
    /// own texture, in the color of its plain row nearest in intensity
    /// (white without one); none for clear.
    pub fn look(&self, kind: Kind, intensity: f32) -> Option<Look> {
        let texture = kind.texture()?;
        let color = self
            .rows
            .iter()
            .filter(|r| r.kind == kind as u32 && r.texture == 0)
            .min_by(|a, b| {
                (a.intensity - intensity)
                    .abs()
                    .total_cmp(&(b.intensity - intensity).abs())
            })
            .map_or([1.0; 3], |r| r.color);
        Some(Look { texture, color })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weather_looks_as_its_kind_in_its_nearest_plain_row_s_color() {
        let w = Weathers {
            rows: vec![
                Row {
                    kind: 1,
                    intensity: 0.25,
                    texture: 0,
                    color: [0.9; 3],
                },
                Row {
                    kind: 1,
                    intensity: 1.0,
                    texture: 344_365,
                    color: [1.0, 0.5, 0.0],
                },
            ],
        };
        // (the red rain is a zone's own: not the plain rain's)
        assert_eq!(
            w.look(Kind::Rain, 1.0),
            Some(Look {
                texture: 186_226,
                color: [0.9; 3]
            })
        );
        assert_eq!(
            w.look(Kind::Snow, 0.5),
            Some(Look {
                texture: 186_228,
                color: [1.0; 3]
            })
        );
        assert_eq!(w.look(Kind::Clear, 1.0), None);
        assert_eq!(Kind::named("sandstorm"), Some(Kind::Sandstorm));
        assert_eq!(Kind::of(4), None);
    }
}
