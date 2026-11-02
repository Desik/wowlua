//! The ground's clutter (decision T560): the small grass, flowers and
//! stones a terrain texture's layer scatters (`MCLY`'s effect), from the
//! client's `GroundEffectTexture` (how dense, which models with what
//! weights) and `GroundEffectDoodad` (each model's file, its size and turn
//! ranges where the build has them).

use std::collections::HashMap;

use crate::Casc;
use crate::tables::Table;

/// A clutter model: its file, how much it is picked, its size and turn
/// (degrees) ranges.
#[derive(Clone, Debug, PartialEq)]
pub struct Doodad {
    pub file: u32,
    pub weight: u32,
    pub size: (f32, f32),
    pub turn: (f32, f32),
}

/// A layer's effect: how dense, and its models.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub density: u32,
    pub doodads: Vec<Doodad>,
}

impl Effect {
    /// A model by a pick from 0 to 1, by the weights.
    pub fn pick(&self, at: f32) -> Option<&Doodad> {
        let total: u32 = self.doodads.iter().map(|d| d.weight.max(1)).sum();
        let mut left = (at.clamp(0.0, 0.9999) * total as f32) as u32;
        for d in &self.doodads {
            let w = d.weight.max(1);
            if left < w {
                return Some(d);
            }
            left -= w;
        }
        self.doodads.last()
    }
}

/// The build's ground effects.
#[derive(Debug, Default)]
pub struct Clutter {
    effects: HashMap<u32, Effect>,
}

impl Clutter {
    /// Reads the build's tables; none where it lacks them.
    pub fn read(casc: &Casc) -> Option<Clutter> {
        let textures = Table::open(casc, "GroundEffectTexture")?;
        let doodads = Table::open(casc, "GroundEffectDoodad")?;
        let mut out = Clutter::default();
        for r in textures.rows() {
            let ids = r.ints("DoodadID");
            // (bytes, read with more above them)
            let weights: Vec<u32> = r.ints("DoodadWeight").iter().map(|w| w & 0xFF).collect();
            let list: Vec<Doodad> = ids
                .iter()
                .enumerate()
                .filter(|(_, id)| (**id & 0xFFFF) != 0)
                .filter_map(|(i, id)| {
                    let d = doodads.row(id & 0xFFFF)?;
                    let file = d.int("ModelFileID");
                    let range = |a: &str, b: &str, none: (f32, f32)| {
                        if doodads.has(a) {
                            let (lo, hi) = (d.float(a), d.float(b));
                            if lo > 0.0 || hi > 0.0 {
                                (lo, hi.max(lo))
                            } else {
                                none
                            }
                        } else {
                            none
                        }
                    };
                    (file != 0).then(|| Doodad {
                        file,
                        weight: weights.get(i).copied().unwrap_or(1),
                        size: range("Size_variation_min", "Size_variation_max", (1.0, 1.0)),
                        turn: range(
                            "Rotation_variation_min",
                            "Rotation_variation_max",
                            (0.0, 360.0),
                        ),
                    })
                })
                .collect();
            if !list.is_empty() {
                out.effects.insert(
                    r.id(),
                    Effect {
                        density: r.int("Density"),
                        doodads: list,
                    },
                );
            }
        }
        Some(out)
    }

    pub fn effect(&self, id: u32) -> Option<&Effect> {
        self.effects.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_are_picked_by_weight() {
        let d = |file, weight| Doodad {
            file,
            weight,
            size: (1.0, 1.0),
            turn: (0.0, 360.0),
        };
        let e = Effect {
            density: 8,
            doodads: vec![d(1, 3), d(2, 1)],
        };
        assert_eq!(e.pick(0.0).unwrap().file, 1);
        assert_eq!(e.pick(0.7).unwrap().file, 1);
        assert_eq!(e.pick(0.8).unwrap().file, 2);
    }
}
