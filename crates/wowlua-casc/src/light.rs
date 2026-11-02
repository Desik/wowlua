//! The world's outdoor light (decision T555): the client's `Light`,
//! `LightParams`, `LightData`, `LightSkybox` and `ZoneLight` tables, read
//! once. A place's light is the map's global light, then its zone lights,
//! then the local lights it stands in, each blended in by its weight (1
//! inside its falloff's start, down to 0 at its end; a zone light's over
//! 50 yards across its border); each light's parameters are interpolated
//! between the time keys around the time of day (half-minutes, 0 to 2880;
//! from the last key around midnight to the first: unverified).

use std::collections::HashMap;

use crate::Casc;
use crate::tables::Table;

/// Half-minutes in a day: `LightData.Time`'s range.
pub const DAY: f64 = 2880.0;

/// Which of a light's eight parameter sets (`Light.LightParamsID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Clear = 0,
    ClearUnderwater = 1,
    Storm = 2,
    StormUnderwater = 3,
    Death = 4,
}

/// The light at a place and time: colors 0 to 1, distances in yards.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sky {
    /// The sun's (the moon's at night) light on what faces it.
    pub direct: [f32; 3],
    pub ambient: [f32; 3],
    /// Ambient from the sky's horizon and from the ground (a hemisphere's
    /// two ends); the plain ambient where the build has none.
    pub horizon_ambient: [f32; 3],
    pub ground_ambient: [f32; 3],
    /// The sky's colors from the top down: top, middle, band 1, band 2,
    /// smog, fog.
    pub sky: [[f32; 3]; 6],
    /// The fog's color near and far.
    pub fog: [f32; 3],
    pub end_fog: [f32; 3],
    /// Where the fog starts and where it is whole.
    pub fog_start: f32,
    pub fog_end: f32,
    /// The sun's disc and its shine.
    pub sun: [f32; 3],
    /// Water's colors near and far, the sea's and the rivers' and lakes',
    /// and their alphas (shallow, deep: rivers, then the sea).
    pub ocean_close: [f32; 3],
    pub ocean_far: [f32; 3],
    pub river_close: [f32; 3],
    pub river_far: [f32; 3],
    pub water_alpha: [f32; 4],
    pub glow: f32,
    /// How much of the light a baked shadow takes (0 to 1, decision
    /// T563): `ShadowOpacity`, a gray (unverified).
    pub shadow: f32,
    /// The sky models to draw: file, `LightSkybox.Flags`, weight.
    pub skyboxes: Vec<(u32, u32, f32)>,
}

/// One time key of a parameter set.
#[derive(Clone, Debug, Default)]
struct Key {
    time: f64,
    sky: Sky,
}

#[derive(Clone, Debug, Default)]
struct Params {
    keys: Vec<Key>,
    skybox: Option<(u32, u32)>,
}

#[derive(Clone, Debug)]
struct Light {
    at: [f64; 3],
    start: f64,
    end: f64,
    params: [u32; 8],
}

#[derive(Clone, Debug)]
struct Zone {
    map: u32,
    light: u32,
    z: (f64, f64),
    /// Its border: (north, west) corners in order.
    points: Vec<[f64; 2]>,
}

/// The build's lights.
#[derive(Debug, Default)]
pub struct Lights {
    by_id: HashMap<u32, Light>,
    by_map: HashMap<u32, Vec<u32>>,
    /// Each map's global light (at the origin).
    global: HashMap<u32, u32>,
    params: HashMap<u32, Params>,
    zones: Vec<Zone>,
}

fn color(v: u32) -> [f32; 3] {
    [
        ((v >> 16) & 0xFF) as f32 / 255.0,
        ((v >> 8) & 0xFF) as f32 / 255.0,
        (v & 0xFF) as f32 / 255.0,
    ]
}

fn mix3(a: &mut [f32; 3], b: [f32; 3], w: f32) {
    for i in 0..3 {
        a[i] += (b[i] - a[i]) * w;
    }
}

impl Sky {
    /// Moves toward `other` by `w` (0 to 1); its sky models, if any, join
    /// with their weights scaled, the earlier ones faded by `1 - w`.
    pub fn mix(&mut self, other: &Sky, w: f32) {
        let w = w.clamp(0.0, 1.0);
        for (a, b) in [
            (&mut self.direct, other.direct),
            (&mut self.ambient, other.ambient),
            (&mut self.horizon_ambient, other.horizon_ambient),
            (&mut self.ground_ambient, other.ground_ambient),
            (&mut self.fog, other.fog),
            (&mut self.end_fog, other.end_fog),
            (&mut self.sun, other.sun),
            (&mut self.ocean_close, other.ocean_close),
            (&mut self.ocean_far, other.ocean_far),
            (&mut self.river_close, other.river_close),
            (&mut self.river_far, other.river_far),
        ] {
            mix3(a, b, w);
        }
        for (a, b) in self.sky.iter_mut().zip(other.sky) {
            mix3(a, b, w);
        }
        for (a, b) in self.water_alpha.iter_mut().zip(other.water_alpha) {
            *a += (b - *a) * w;
        }
        self.fog_start += (other.fog_start - self.fog_start) * w;
        self.fog_end += (other.fog_end - self.fog_end) * w;
        self.glow += (other.glow - self.glow) * w;
        self.shadow += (other.shadow - self.shadow) * w;
        if w > 0.0 && !other.skyboxes.is_empty() {
            for s in &mut self.skyboxes {
                s.2 *= 1.0 - w;
            }
            self.skyboxes
                .extend(other.skyboxes.iter().map(|&(f, fl, sw)| (f, fl, sw * w)));
            self.skyboxes.retain(|s| s.2 > 0.001);
        }
    }
}

impl Lights {
    /// Reads the build's tables; none where it lacks them.
    pub fn read(casc: &Casc) -> Option<Lights> {
        let light = Table::open(casc, "Light")?;
        let params = Table::open(casc, "LightParams")?;
        let data = Table::open(casc, "LightData")?;
        let skyboxes = Table::open(casc, "LightSkybox");
        let mut out = Lights::default();
        for r in light.rows() {
            let at = r.floats("GameCoords");
            let ids = r.ints("LightParamsID");
            let mut p = [0u32; 8];
            for (i, v) in ids.iter().take(8).enumerate() {
                p[i] = *v;
            }
            let l = Light {
                at: [
                    f64::from(*at.first().unwrap_or(&0.0)),
                    f64::from(*at.get(1).unwrap_or(&0.0)),
                    f64::from(*at.get(2).unwrap_or(&0.0)),
                ],
                start: f64::from(r.float("GameFalloffStart")),
                end: f64::from(r.float("GameFalloffEnd")),
                params: p,
            };
            let map = r.int("ContinentID");
            // (at the origin: the map's global light, whatever its radius:
            // WoW Forever's Zephras Isle has one of 34 yards)
            if l.at == [0.0; 3] {
                out.global.entry(map).or_insert(r.id());
            } else {
                out.by_map.entry(map).or_default().push(r.id());
            }
            out.by_id.insert(r.id(), l);
        }
        // (a sky model by its file, or where the build names only its path,
        // `….mdx`, the path's `.m2`: Classic Era's)
        let skybox = |id: u32| -> Option<(u32, u32)> {
            let row = skyboxes.as_ref()?.row(id)?;
            let file = match row.int("SkyboxFileDataID") {
                0 => {
                    let name = row.string("Name")?;
                    let stem = name
                        .strip_suffix(".mdx")
                        .or_else(|| name.strip_suffix(".mdl"))?;
                    casc.id_of(&format!("{stem}.m2"))?
                }
                f => f,
            };
            Some((file, row.int("Flags")))
        };
        for r in params.rows() {
            out.params.insert(
                r.id(),
                Params {
                    keys: Vec::new(),
                    skybox: skybox(r.int("LightSkyboxID")),
                },
            );
        }
        let alphas: HashMap<u32, ([f32; 4], f32)> = params
            .rows()
            .map(|r| {
                (
                    r.id(),
                    (
                        [
                            r.float("WaterShallowAlpha"),
                            r.float("WaterDeepAlpha"),
                            r.float("OceanShallowAlpha"),
                            r.float("OceanDeepAlpha"),
                        ],
                        r.float("Glow"),
                    ),
                )
            })
            .collect();
        for r in data.rows() {
            let pid = r.int("LightParamID");
            let Some(p) = out.params.get_mut(&pid) else {
                continue;
            };
            let c = |name: &str| color(r.int(name));
            let ambient = c("AmbientColor");
            let or_ambient = |v: [f32; 3]| if v == [0.0; 3] { ambient } else { v };
            let fog = c("SkyFogColor");
            let end_fog = c("EndFogColor");
            // (the fog's numbers as the client fixes them up; in inches as
            // the old tables' were: 18000 is 500 yards, unverified; none on
            // many of WoW Forever's keys: NaN, filled in below)
            let fog_end = match r.float("FogEnd") {
                e if e > 0.0 => e.max(10.0) / 36.0,
                _ => f32::NAN,
            };
            let fog_start = fog_end * r.float("FogScaler").clamp(-1.0, 1.0);
            let (water_alpha, glow) = alphas.get(&pid).copied().unwrap_or_default();
            let sky = Sky {
                direct: c("DirectColor"),
                ambient,
                horizon_ambient: or_ambient(c("HorizonAmbientColor")),
                ground_ambient: or_ambient(c("GroundAmbientColor")),
                sky: [
                    c("SkyTopColor"),
                    c("SkyMiddleColor"),
                    c("SkyBand1Color"),
                    c("SkyBand2Color"),
                    c("SkySmogColor"),
                    fog,
                ],
                fog,
                end_fog: if end_fog == [0.0; 3] { fog } else { end_fog },
                fog_start: fog_start.max(0.0),
                fog_end,
                sun: c("SunColor"),
                ocean_close: c("OceanCloseColor"),
                ocean_far: c("OceanFarColor"),
                river_close: c("RiverCloseColor"),
                river_far: c("RiverFarColor"),
                water_alpha,
                glow,
                shadow: {
                    let s = c("ShadowOpacity");
                    (s[0] + s[1] + s[2]) / 3.0
                },
                skyboxes: p
                    .skybox
                    .map(|(f, fl)| vec![(f, fl, 1.0)])
                    .unwrap_or_default(),
            };
            p.keys.push(Key {
                // (16 bits: WoW Forever's layout reads with more above)
                time: f64::from(r.int("Time") & 0xFFFF),
                sky,
            });
        }
        for p in out.params.values_mut() {
            p.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
            fill_fog(&mut p.keys);
        }
        if let Some(zones) = Table::open(casc, "ZoneLight") {
            let points = Table::open(casc, "ZoneLightPoint");
            for r in zones.rows() {
                let mut pts: Vec<(u32, [f64; 2])> = points
                    .as_ref()
                    .map(|t| {
                        t.rows_where("ZoneLightID", r.id())
                            .iter()
                            .map(|p| {
                                let pos = p.floats("Pos");
                                (
                                    p.int("PointOrder"),
                                    [
                                        f64::from(*pos.first().unwrap_or(&0.0)),
                                        f64::from(*pos.get(1).unwrap_or(&0.0)),
                                    ],
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                pts.sort_by_key(|p| p.0);
                if pts.len() >= 3 {
                    out.zones.push(Zone {
                        map: r.int("MapID"),
                        light: r.int("LightID"),
                        z: (f64::from(r.float("Zmin")), f64::from(r.float("Zmax"))),
                        points: pts.into_iter().map(|p| p.1).collect(),
                    });
                }
            }
        }
        Some(out)
    }

    /// A parameter set at a time of day (half-minutes).
    fn params_at(&self, id: u32, time: f64) -> Option<Sky> {
        let keys = &self.params.get(&id)?.keys;
        let first = keys.first()?;
        if keys.len() == 1 {
            return Some(first.sky.clone());
        }
        let t = time.rem_euclid(DAY);
        let after = keys.iter().position(|k| k.time > t);
        let (a, b, span, into) = match after {
            Some(0) | None => {
                // across midnight: the last key to the first
                let last = keys.last()?;
                let span = (first.time + DAY - last.time).max(1.0);
                let into = (t - last.time).rem_euclid(DAY);
                (last, first, span, into)
            }
            Some(i) => {
                let (a, b) = (&keys[i - 1], &keys[i]);
                (a, b, (b.time - a.time).max(1.0), t - a.time)
            }
        };
        let mut sky = a.sky.clone();
        let skyboxes = sky.skyboxes.clone();
        sky.mix(&b.sky, (into / span) as f32);
        // (a light's sky models are its own, not faded by its own keys)
        sky.skyboxes = skyboxes;
        Some(sky)
    }

    fn light_at(&self, id: u32, kind: Kind, time: f64) -> Option<Sky> {
        let l = self.by_id.get(&id)?;
        let p = match l.params[kind as usize] {
            0 => l.params[Kind::Clear as usize],
            p => p,
        };
        self.params_at(p, time)
    }

    /// The light at a place of a map (north, west, up) and a time of day.
    pub fn at(&self, map: u32, at: [f64; 3], time: f64, kind: Kind) -> Option<Sky> {
        let global = self
            .global
            .get(&map)
            .or_else(|| self.global.get(&0))
            .copied()?;
        let mut sky = self.light_at(global, kind, time)?;
        for z in self.zones.iter().filter(|z| z.map == map) {
            if at[2] < z.z.0 || at[2] > z.z.1 {
                continue;
            }
            let w = zone_weight(&z.points, [at[0], at[1]]);
            if w > 0.0
                && let Some(s) = self.light_at(z.light, kind, time)
            {
                sky.mix(&s, w as f32);
            }
        }
        let mut local: Vec<(f64, u32)> = self
            .by_map
            .get(&map)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| {
                        let l = &self.by_id[id];
                        let d = ((l.at[0] - at[0]).powi(2)
                            + (l.at[1] - at[1]).powi(2)
                            + (l.at[2] - at[2]).powi(2))
                        .sqrt();
                        if d >= l.end {
                            return None;
                        }
                        let w = if d <= l.start {
                            1.0
                        } else {
                            1.0 - (d - l.start) / (l.end - l.start).max(1.0)
                        };
                        Some((w.clamp(0.0, 1.0), *id))
                    })
                    .collect()
            })
            .unwrap_or_default();
        // the weaker first, the strongest last
        local.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (w, id) in local {
            if let Some(s) = self.light_at(id, kind, time) {
                sky.mix(&s, w as f32);
            }
        }
        Some(sky)
    }
}

/// A key without a fog distance (WoW Forever's, whose fog its volume fog
/// table gives) takes the nearest earlier key's, around midnight, else 125
/// to 500 yards (unverified).
fn fill_fog(keys: &mut [Key]) {
    let known: Vec<(usize, f32, f32)> = keys
        .iter()
        .enumerate()
        .filter(|(_, k)| !k.sky.fog_end.is_nan())
        .map(|(i, k)| (i, k.sky.fog_start, k.sky.fog_end))
        .collect();
    for (i, key) in keys.iter_mut().enumerate() {
        if !key.sky.fog_end.is_nan() {
            continue;
        }
        let (start, end) = known
            .iter()
            .rev()
            .find(|(j, _, _)| *j < i)
            .or_else(|| known.last())
            .map_or((125.0, 500.0), |&(_, s, e)| (s, e));
        key.sky.fog_start = start;
        key.sky.fog_end = end;
    }
}

/// How far into a zone light's border a point is: 1 well inside, 0.5 on
/// the border, 0 more than 25 yards outside (a 50-yard ramp).
fn zone_weight(points: &[[f64; 2]], p: [f64; 2]) -> f64 {
    let mut inside = false;
    let mut nearest = f64::MAX;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx * dx + dy * dy;
        let t = if len > 0.0 {
            (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (cx, cy) = (a[0] + t * dx, a[1] + t * dy);
        nearest = nearest.min(((p[0] - cx).powi(2) + (p[1] - cy).powi(2)).sqrt());
    }
    let signed = if inside { nearest } else { -nearest };
    (0.5 + signed / 50.0).clamp(0.0, 1.0)
}

/// The direction the sun's (the moon's) light travels at a time of day
/// (half-minutes), north, west, up: the client's direct light, its angle
/// from straight up 127° at midnight and noon and 110° at six, from the
/// south-east (unverified beyond the open viewers' tables).
pub fn light_direction(time: f64) -> [f64; 3] {
    let f = (time / DAY).rem_euclid(1.0);
    // (piecewise linear: 0 → 127°, 0.25 → 110°, 0.5 → 127°, 0.75 → 110°)
    let quarter = (f * 4.0).fract();
    let rising = ((f * 4.0) as u32).is_multiple_of(2);
    let (a, b) = if rising {
        (127.0, 110.0)
    } else {
        (110.0, 127.0)
    };
    let phi = f64::to_radians(a + (b - a) * quarter);
    let theta = f64::to_radians(225.0);
    [phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zone_light_s_weight_ramps_across_its_border() {
        let square = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        assert_eq!(zone_weight(&square, [50.0, 50.0]), 1.0);
        assert!((zone_weight(&square, [100.0, 50.0]) - 0.5).abs() < 1e-9);
        assert_eq!(zone_weight(&square, [200.0, 50.0]), 0.0);
    }

    #[test]
    fn the_light_comes_from_above() {
        for t in [0.0, 720.0, 1440.0, 2160.0] {
            assert!(light_direction(t)[2] < 0.0, "{t}");
        }
        let six = light_direction(720.0);
        assert!((six[2] - f64::to_radians(110.0).cos()).abs() < 1e-9);
    }

    #[test]
    fn skies_mix() {
        let mut a = Sky {
            fog_end: 100.0,
            skyboxes: vec![(1, 0, 1.0)],
            ..Sky::default()
        };
        let b = Sky {
            fog_end: 300.0,
            direct: [1.0, 1.0, 1.0],
            skyboxes: vec![(2, 0, 1.0)],
            ..Sky::default()
        };
        a.mix(&b, 0.5);
        assert_eq!(a.fog_end, 200.0);
        assert_eq!(a.direct, [0.5, 0.5, 0.5]);
        assert_eq!(a.skyboxes, vec![(1, 0, 0.5), (2, 0, 0.5)]);
    }
}
