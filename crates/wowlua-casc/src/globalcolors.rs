//! The build's global colors (decision T554): the client's `GlobalColor`
//! table, which `C_UIColor.GetColors` answers and Blizzard's code makes
//! its color globals of (`NORMAL_FONT_COLOR`).

use crate::Casc;
use crate::tables::Table;

/// The build's colors, `(name, 0xAARRGGBB)`, in the order the client
/// answers them: by name, byte by byte.
pub fn read(casc: &Casc) -> Option<Vec<(String, u32)>> {
    let table = Table::open(casc, "GlobalColor")?;
    let mut out: Vec<(String, u32)> = table
        .rows()
        .filter_map(|r| Some((r.string("LuaConstantName")?, r.int("Color"))))
        .filter(|(name, _)| !name.is_empty())
        .collect();
    out.sort();
    out.dedup_by(|a, b| a.0 == b.0);
    Some(out)
}
