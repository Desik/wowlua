//! What a spell looks like in the world (decision T582): its visual
//! (`SpellXSpellVisual`), the visual's events (`SpellVisualEvent`: when
//! each kit plays and on whom), the kits' models at attachment points
//! (`SpellVisualKitEffect` of type 2, `SpellVisualKitModelAttach`,
//! `SpellVisualEffectName`), its missiles (`SpellVisualMissile`) and how
//! fast they fly (`SpellMisc`'s `Speed`). Each build's own tables: the
//! same spell looks different per flavor (Fireball's impact).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::Casc;
use crate::tables::{Row, Table};

/// When a kit plays, by its event's start and end (the values read from
/// the tables: unverified names). Precast 1 to 2 (the cast bar), cast 3
/// (the spell going off), impact 6 (where it lands), aura 7 to 8 (while
/// up), channel 11 to 12; 13 ends a one-shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    Precast,
    Cast,
    Impact,
    Aura,
    Channel,
}

impl Phase {
    fn of(start: u32) -> Option<Phase> {
        match start {
            1 => Some(Phase::Precast),
            3 => Some(Phase::Cast),
            6 => Some(Phase::Impact),
            7 => Some(Phase::Aura),
            11 => Some(Phase::Channel),
            _ => None,
        }
    }
}

/// A model a kit puts on a unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Attach {
    /// The `.m2` file.
    pub model: u32,
    /// The unit's attachment point (-1: none, at its feet).
    pub attachment: i32,
    /// Its size (the attach's and its effect name's).
    pub scale: f32,
    /// Where from the point (yards) and how turned (radians: yaw,
    /// pitch, roll).
    pub offset: [f32; 3],
    pub turn: [f32; 3],
    /// How long after its event it shows (seconds; unverified unit).
    pub delay: f32,
    /// On the caster (the event's target type 1), else on the target (2).
    pub on_caster: bool,
}

/// A missile a spell flies from its caster to its target.
#[derive(Clone, Debug, PartialEq)]
pub struct Missile {
    pub model: u32,
    pub scale: f32,
    /// The caster's and the target's attachment points (-1: none named).
    pub from: i32,
    pub to: i32,
}

/// A spell's look.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpellLook {
    pub effects: Vec<(Phase, Attach)>,
    pub missiles: Vec<Missile>,
    /// Its missiles' speed (yards a second; 0 none: it lands at once).
    pub speed: f32,
}

impl SpellLook {
    /// Its models for a phase.
    pub fn of(&self, phase: Phase) -> impl Iterator<Item = &Attach> {
        self.effects
            .iter()
            .filter(move |(p, _)| *p == phase)
            .map(|(_, a)| a)
    }
}

/// The build's spell visuals: the tables, read on first use; each spell's
/// look kept once read.
pub struct SpellVisuals {
    x: Table,
    visual: Table,
    events: Table,
    kit_effects: Table,
    attaches: Table,
    names: Table,
    missiles: Option<Table>,
    misc: Option<Table>,
    looks: Mutex<HashMap<u32, Arc<SpellLook>>>,
}

impl std::fmt::Debug for SpellVisuals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpellVisuals").finish_non_exhaustive()
    }
}

/// `SpellVisualKitEffect`'s `EffectType` for a model at an attachment.
const MODEL_ATTACH: u32 = 2;

impl SpellVisuals {
    /// Opens the build's tables; none where it lacks one of them.
    pub fn read(casc: &Casc) -> Option<SpellVisuals> {
        Some(SpellVisuals {
            x: Table::open(casc, "SpellXSpellVisual")?,
            visual: Table::open(casc, "SpellVisual")?,
            events: Table::open(casc, "SpellVisualEvent")?,
            kit_effects: Table::open(casc, "SpellVisualKitEffect")?,
            attaches: Table::open(casc, "SpellVisualKitModelAttach")?,
            names: Table::open(casc, "SpellVisualEffectName")?,
            missiles: Table::open(casc, "SpellVisualMissile"),
            misc: Table::open(casc, "SpellMisc"),
            looks: Mutex::new(HashMap::new()),
        })
    }

    /// A spell's look: its visual for the base difficulty and anyone
    /// (the first row naming no difficulty and no condition, else its
    /// first); an empty one for a spell without.
    pub fn look(&self, spell: u32) -> Arc<SpellLook> {
        if let Some(l) = self.looks.lock().ok().and_then(|m| m.get(&spell).cloned()) {
            return l;
        }
        let look = Arc::new(self.read_look(spell));
        if let Ok(mut m) = self.looks.lock() {
            m.insert(spell, look.clone());
        }
        look
    }

    fn read_look(&self, spell: u32) -> SpellLook {
        let rows = self.x.rows_where("SpellID", spell);
        let plain = |r: &Row<'_>| {
            [
                "DifficultyID",
                "CasterUnitConditionID",
                "CasterPlayerConditionID",
                "ViewerUnitConditionID",
                "ViewerPlayerConditionID",
            ]
            .iter()
            .all(|c| r.int(c) == 0)
        };
        let Some(visual) = rows
            .iter()
            .find(|r| plain(r))
            .or(rows.first())
            .map(|r| r.int("SpellVisualID"))
        else {
            return SpellLook::default();
        };
        let mut look = SpellLook::default();
        for e in self.events.rows_where("SpellVisualID", visual) {
            let Some(phase) = Phase::of(e.int("StartEvent")) else {
                continue;
            };
            let on_caster = e.int("TargetType") == 1;
            for a in self.kit(e.int("SpellVisualKitID"), on_caster) {
                // (an impact's or an aura's on the caster and the target
                // alike: the same model once, T582)
                let twin = matches!(phase, Phase::Impact | Phase::Aura)
                    && look.effects.iter().any(|(p, b)| {
                        *p == phase && b.model == a.model && b.attachment == a.attachment
                    });
                if !twin {
                    look.effects.push((phase, a));
                }
            }
        }
        if let (Some(v), Some(missiles)) = (self.visual.row(visual), &self.missiles) {
            let set = v.int("SpellVisualMissileSetID");
            if set != 0 {
                for m in missiles.rows_where("SpellVisualMissileSetID", set) {
                    if let Some((model, scale)) = self.effect_name(m.int("SpellVisualEffectNameID"))
                    {
                        look.missiles.push(Missile {
                            model,
                            scale,
                            from: m.signed("Attachment"),
                            to: m.signed("DestinationAttachment"),
                        });
                    }
                }
            }
        }
        if let Some(misc) = &self.misc {
            let rows = misc.rows_where("SpellID", spell);
            look.speed = rows
                .iter()
                .find(|r| !misc.has("DifficultyID") || r.int("DifficultyID") == 0)
                .or(rows.first())
                .map_or(0.0, |r| r.float("Speed"))
                .max(0.0);
        }
        look
    }

    /// A kit's models at attachment points.
    fn kit(&self, kit: u32, on_caster: bool) -> Vec<Attach> {
        let mut out = Vec::new();
        for k in self.kit_effects.rows_where("ParentSpellVisualKitID", kit) {
            if k.int("EffectType") != MODEL_ATTACH {
                continue;
            }
            let Some(a) = self.attaches.row(k.int("Effect")) else {
                continue;
            };
            let Some((model, scale)) = self.effect_name(a.int("SpellVisualEffectNameID")) else {
                continue;
            };
            let offset = a.floats("Offset");
            let s = a.float("Scale");
            out.push(Attach {
                model,
                attachment: a.signed("AttachmentID"),
                scale: scale * if s > 0.0 { s } else { 1.0 },
                offset: [0, 1, 2].map(|i| offset.get(i).copied().unwrap_or(0.0)),
                turn: [a.float("Yaw"), a.float("Pitch"), a.float("Roll")],
                delay: a.float("StartDelay").max(0.0),
                on_caster,
            });
        }
        out
    }

    /// An effect name's model and size: a model file's only (its type 0).
    fn effect_name(&self, id: u32) -> Option<(u32, f32)> {
        let n = self.names.row(id)?;
        let model = n.int("ModelFileDataID");
        if model == 0 || n.int("Type") != 0 {
            return None;
        }
        let s = n.float("Scale");
        Some((model, if s > 0.0 { s } else { 1.0 }))
    }
}
