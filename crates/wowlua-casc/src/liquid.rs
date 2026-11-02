//! What a liquid is (decision T557): the client's `LiquidType`,
//! `LiquidObject`, `LiquidMaterial` and `LiquidTypeXTexture` tables, read
//! once: a terrain liquid's kind, how its vertices are stored, how it
//! flows, and its surface's animation frames.

use std::collections::HashMap;

use crate::Casc;
use crate::adt::Liquid;
use crate::tables::Table;

/// How a liquid is drawn (`LiquidMaterial`'s ID): water, the sea's too,
/// or a glowing one (magma, slime) that needs no light; others as water.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Water,
    Glowing,
    /// WoW Forever's PBR water (material 130): its own look (decision
    /// T583), from its foam masks.
    Pbr,
}

/// A liquid type.
#[derive(Clone, Debug, PartialEq)]
pub struct Kind {
    pub id: u32,
    pub name: String,
    pub material: u32,
    pub look: Look,
    /// The vertex format its material stores (`LiquidMaterial.LVF`).
    pub format: u32,
    /// The surface's animation frames (file IDs); a PBR water's foam masks
    /// in their order (decision T583: three churning, the shore's two, the
    /// river's).
    pub frames: Vec<u32>,
    /// The sea (type 2): flat, its depth alone.
    pub ocean: bool,
    /// Its own colors (`Color`, WoW Forever's PBR water's: near, far; which
    /// is which unverified), else none: the light's.
    pub colors: Vec<[f32; 3]>,
}

/// The build's liquids.
#[derive(Debug, Default)]
pub struct Liquids {
    kinds: HashMap<u32, Kind>,
    /// `LiquidObject`: its type, flow direction (radians) and speed.
    objects: HashMap<u32, (u32, f32, f32)>,
}

/// A terrain liquid as its tables say: its kind, vertex format and flow.
#[derive(Clone, Debug)]
pub struct Resolved<'a> {
    pub kind: &'a Kind,
    pub format: u32,
    pub flow: (f32, f32),
}

impl Liquids {
    /// Reads the build's tables; none where it lacks them.
    pub fn read(casc: &Casc) -> Option<Liquids> {
        let types = Table::open(casc, "LiquidType")?;
        let materials = Table::open(casc, "LiquidMaterial");
        let textures = Table::open(casc, "LiquidTypeXTexture");
        let mut out = Liquids::default();
        for r in types.rows() {
            let material = r.int("MaterialID");
            let format = materials
                .as_ref()
                .and_then(|m| m.row(material))
                .map_or(0, |m| m.int("LVF"));
            // (bytes, read with more above them; at most 64 frames)
            let count =
                (r.ints("FrameCountTexture").first().copied().unwrap_or(0) & 0xFF).min(64) as usize;
            // The first texture's frames: the build's list where it has
            // one, else its name's pattern (`…lake_a.%d.blp`, 1 up).
            let mut frames: Vec<(u32, u32)> = textures
                .as_ref()
                .map(|t| {
                    t.rows_where("LiquidTypeID", r.id())
                        .iter()
                        .map(|x| (x.int("OrderIndex"), x.int("FileDataID")))
                        .collect()
                })
                .unwrap_or_default();
            frames.sort();
            // (a PBR water's every mask, T583)
            let keep = if material == 130 {
                frames.len()
            } else {
                count.max(1)
            };
            let mut frames: Vec<u32> = frames.into_iter().map(|f| f.1).take(keep).collect();
            frames.retain(|f| *f != 0);
            if frames.is_empty()
                && let Some(pattern) = r.string("Texture").filter(|s| s.contains("%d"))
            {
                frames = (1..=count.max(1))
                    .filter_map(|i| casc.id_of(&pattern.replace("%d", &i.to_string())))
                    .collect();
            }
            let look = match material {
                2 | 4 => Look::Glowing,
                130 => Look::Pbr,
                _ => Look::Water,
            };
            out.kinds.insert(
                r.id(),
                Kind {
                    id: r.id(),
                    name: r.string("Name").unwrap_or_default(),
                    material,
                    look,
                    format,
                    frames,
                    ocean: r.id() == 2,
                    colors: r
                        .ints("Color")
                        .into_iter()
                        .filter(|c| c & 0xFF_FFFF != 0)
                        .map(|c| {
                            [(c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF].map(|v| v as f32 / 255.0)
                        })
                        .collect(),
                },
            );
        }
        if let Some(objects) = Table::open(casc, "LiquidObject") {
            for r in objects.rows() {
                out.objects.insert(
                    r.id(),
                    (
                        // (16 bits: WoW Forever's has others above them,
                        // 0x3D4C0000 over every row; T583)
                        r.int("LiquidTypeID") & 0xFFFF,
                        r.float("FlowDirection"),
                        r.float("FlowSpeed"),
                    ),
                );
            }
        }
        Some(out)
    }

    pub fn kind(&self, id: u32) -> Option<&Kind> {
        self.kinds.get(&id)
    }

    /// A terrain liquid's kind, its vertex format (the sea's: its depth
    /// alone) and its flow.
    pub fn resolve(&self, l: &Liquid) -> Option<Resolved<'_>> {
        let (kind, flow) = if l.object >= 42 {
            let &(kind, dir, speed) = self.objects.get(&l.object)?;
            (kind, (dir, speed))
        } else {
            (l.kind, (0.0, 0.0))
        };
        let kind = self.kinds.get(&kind).or_else(|| self.kinds.get(&l.kind))?;
        let format = if kind.ocean {
            2
        } else if l.object < 42 {
            l.object
        } else {
            kind.format
        };
        Some(Resolved { kind, format, flow })
    }
}
