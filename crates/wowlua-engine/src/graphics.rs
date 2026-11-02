//! The Graphics settings (decision T561): a setting of the Graphics panel
//! taking a level sets the CVars the client sets for it (`shadowMode`,
//! `shadowNumCascades`, …), and the overall quality (`graphicsQuality`)
//! sets each setting's level, as the client does: `data/camelot-graphics.txt`
//! (`/stdtest engine graphics`), else for the overall quality the recorded
//! `GetGraphicsCVarValueForQualityLevel` answers. Other flavors' are not
//! recorded yet.

use std::collections::HashMap;
use std::sync::OnceLock;

use wowlua_vm::Vm;

use crate::engine;

/// `setting=level` to the CVars it sets.
type Levels = HashMap<(String, u32), Vec<(String, String)>>;

fn levels(flavor: &str) -> Option<&'static Levels> {
    static CAMELOT: OnceLock<Levels> = OnceLock::new();
    (flavor == "camelot")
        .then(|| CAMELOT.get_or_init(|| parse(include_str!("../data/camelot-graphics.txt"))))
}

fn parse(text: &str) -> Levels {
    let mut out = Levels::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let mut words = line.split_whitespace();
        let Some((name, level)) = words.next().and_then(|w| w.split_once('=')) else {
            continue;
        };
        let Ok(level) = level.parse::<u32>() else {
            continue;
        };
        // (a level the client refused: none)
        if line.contains(" refused") {
            continue;
        }
        let sets = words
            .filter_map(|w| w.split_once('='))
            .filter(|(k, _)| !k.eq_ignore_ascii_case(name))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        out.insert((name.to_ascii_lowercase(), level), sets);
    }
    out
}

/// The settings the overall quality sets (the Graphics panel's).
const SETTINGS: [&str; 15] = [
    "graphicsShadowQuality",
    "graphicsViewDistance",
    "graphicsGroundClutter",
    "graphicsEnvironmentDetail",
    "graphicsLiquidDetail",
    "graphicsPBRLiquidDetail",
    "graphicsParticleDensity",
    "graphicsSpellDensity",
    "graphicsSSAO",
    "graphicsDepthEffects",
    "graphicsComputeEffects",
    "graphicsOutlineMode",
    "graphicsProjectedTextures",
    "graphicsTextureResolution",
    "graphicsLightMode",
];

/// At the start: each setting's CVars as its level is, as the client sets
/// them when it starts (its registered defaults disagree: Shadow Quality
/// 4 with `shadowMode` 0), not told.
pub(crate) fn settle(vm: &mut Vm) {
    for setting in SETTINGS {
        if let Some(v) = crate::framescript::cvar_get(vm, setting) {
            apply(vm, setting, &v, false);
        }
    }
}

/// After `name` took `value` (`C_CVar.SetCVar`): the CVars it sets, set
/// and told as the client does.
pub(crate) fn follow(vm: &mut Vm, name: &str, value: &str) {
    apply(vm, name, value, true);
}

fn apply(vm: &mut Vm, name: &str, value: &str, tell: bool) {
    let Some(table) = levels(&engine(vm).flavor) else {
        return;
    };
    let Ok(level) = value.trim().parse::<u32>() else {
        return;
    };
    let key = (name.to_ascii_lowercase(), level);
    if let Some(sets) = table.get(&key) {
        for (k, v) in sets.clone() {
            let _ = crate::framescript::set_cvar_text(vm, &k, v, tell);
            if !tell {
                let now = crate::framescript::cvar_get(vm, &k).unwrap_or_default();
                apply(vm, &k, &now, false);
            }
        }
        return;
    }
    // The overall quality without a recording of its own: each setting's
    // level as the client answers it (by the overall quality's value: its
    // default 6 gives the Shadow Quality default 4), each setting then
    // following.
    if name.eq_ignore_ascii_case("graphicsQuality") {
        for setting in SETTINGS {
            let call =
                format!("GetGraphicsCVarValueForQualityLevel(\"{setting}\", {level}, false)");
            let Some(answer) = crate::recorded::answers().get(call.as_str()) else {
                continue;
            };
            if let Some(n) = answer.strip_prefix("1: number ") {
                let _ = crate::framescript::set_cvar_text(vm, setting, n.trim().to_string(), tell);
                if !tell {
                    apply(vm, setting, n.trim(), false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_s_cvars_are_read() {
        let t = parse(
            "# x\ngraphicsShadowQuality=4 graphicsShadowQuality=4 shadowMode=3 shadowSoft=1\ngraphicsSSAO=9 refused (4)\n",
        );
        assert_eq!(
            t[&("graphicsshadowquality".to_string(), 4)],
            vec![
                ("shadowMode".to_string(), "3".to_string()),
                ("shadowSoft".to_string(), "1".to_string())
            ]
        );
        assert!(!t.contains_key(&("graphicsssao".to_string(), 9)));
    }
}
