//! The build's global strings (decision T553): the client's
//! `GlobalStrings` table, which the game sets as globals before its UI
//! code loads (`OKAY = "Okay"`).

use crate::Casc;
use crate::tables::Table;

/// Which screens a string is set on (the table's `Flags`).
pub const IN_GAME: u32 = 1;
pub const AT_GLUE: u32 = 2;

/// The build's global strings, `(name, text, flags)`, by name.
pub fn read(casc: &Casc) -> Option<Vec<(String, String, u32)>> {
    let table = Table::open(casc, "GlobalStrings")?;
    let mut out: Vec<(String, String, u32)> = table
        .rows()
        .filter_map(|r| {
            Some((
                r.string("BaseTag")?,
                r.string("TagText_lang")?,
                r.int("Flags"),
            ))
        })
        .filter(|(name, _, _)| !name.is_empty())
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.dedup_by(|a, b| a.0 == b.0);
    Some(out)
}

/// The strings set in the game as a Lua file, written as Ketho's
/// BlizzardInterfaceResources writes `GlobalStrings/enUS.lua`
/// (`OKAY = "Okay";`, `_G["101_X"] = "…";`).
pub fn lua(strings: &[(String, String, u32)]) -> String {
    let mut out = String::new();
    for (name, text, _) in strings.iter().filter(|(_, _, f)| f & IN_GAME != 0) {
        let plain = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if plain {
            out.push_str(name);
        } else {
            out.push_str(&format!("_G[{}]", quoted(name)));
        }
        out.push_str(" = ");
        out.push_str(&quoted(text));
        out.push_str(";\n");
    }
    out
}

fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\{}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_are_written_as_lua() {
        let strings = vec![
            ("OKAY".to_string(), "Okay".to_string(), IN_GAME),
            (
                "101_X".to_string(),
                "a \"b\"\nc\\d".to_string(),
                IN_GAME | AT_GLUE,
            ),
            ("LOGIN".to_string(), "Login".to_string(), AT_GLUE),
        ];
        assert_eq!(
            lua(&strings),
            "OKAY = \"Okay\";\n_G[\"101_X\"] = \"a \\\"b\\\"\\nc\\\\d\";\n"
        );
    }
}
