//! The build's global strings (decision T553): the client's
//! `GlobalStrings` table, which the game sets as globals before its UI
//! code loads (`OKAY = "Okay"`).

use crate::Casc;
use crate::tables::Table;

/// Which screens a string is set on (the table's `Flags`).
pub const IN_GAME: u32 = 1;
pub const AT_GLUE: u32 = 2;

/// The build's global strings, `(name, text, flags)`, by name. The table
/// writes its texts as in Lua source (`\n`, `\32`, `\\`); they are read as
/// the game sets them (Ketho's dumps of the game's values agree).
pub fn read(casc: &Casc) -> Option<Vec<(String, String, u32)>> {
    let table = Table::open(casc, "GlobalStrings")?;
    let mut out: Vec<(String, String, u32)> = table
        .rows()
        .filter_map(|r| {
            Some((
                r.string("BaseTag")?,
                unescape(&r.string("TagText_lang")?),
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

/// A text written as in a Lua string read: `\n`, `\t`, `\r`, `\\`, `\"`,
/// `\'` and `\ddd` (a byte); any other backslash kept.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let c = b[i + 1];
        let simple = match c {
            b'n' => Some(b'\n'),
            b't' => Some(b'\t'),
            b'r' => Some(b'\r'),
            b'\\' | b'"' | b'\'' => Some(c),
            _ => None,
        };
        let digits = b[i + 1..]
            .iter()
            .take(3)
            .take_while(|d| d.is_ascii_digit())
            .count();
        let byte = (digits > 0)
            .then(|| s[i + 1..i + 1 + digits].parse::<u8>().ok())
            .flatten();
        if let Some(e) = simple {
            out.push(e);
            i += 2;
        } else if let Some(n) = byte {
            out.push(n);
            i += 1 + digits;
        } else {
            out.push(b'\\');
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
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

    /// The table's texts as the game has them (Ketho's dump of camelot's:
    /// `CHAT_SAY_GET = "%s says: "`, `|TInterface\GroupFrame\…|t`).
    #[test]
    fn the_tables_escapes_are_read() {
        assert_eq!(unescape(r"%s says:\32"), "%s says: ");
        assert_eq!(
            unescape(r#"|TInterface\\Icons\\X:16|t a\nb \"q\" \x"#),
            "|TInterface\\Icons\\X:16|t a\nb \"q\" \\x"
        );
        assert_eq!(unescape(r"end\"), "end\\");
        assert_eq!(unescape(r"\999"), "\\999");
    }

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
