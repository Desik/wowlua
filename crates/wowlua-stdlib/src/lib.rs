//! The wowlua standard library (spec chapter 8): `wowlua:*` modules,
//! written in wowlua and compiled into each addon that imports them, and the
//! compiler's implicit declarations.

/// A file shipped inside the compiler.
#[derive(Clone, Copy, Debug)]
pub struct File {
    /// Its path as diagnostics show it (`<wowlua>/widgets.d.wowlua`).
    pub path: &'static str,
    pub text: &'static str,
}

/// Declarations in scope in every package: Lua 5.1 and WoW's Lua
/// environment, the language's own types, and FrameXML globals the
/// generated wow-api declarations lack.
pub fn declarations() -> &'static [File] {
    &[
        File {
            path: "<wowlua>/lua.d.wowlua",
            text: include_str!("../decl/lua.d.wowlua"),
        },
        File {
            path: "<wowlua>/wowlua.d.wowlua",
            text: include_str!("../decl/wowlua.d.wowlua"),
        },
        File {
            path: "<wowlua>/wow.d.wowlua",
            text: include_str!("../decl/wow.d.wowlua"),
        },
        File {
            path: "<wowlua>/runtime.d.wowlua",
            text: include_str!("../decl/runtime.d.wowlua"),
        },
    ]
}

/// A `wowlua:*` module: its wowlua source and, optionally, a declaration
/// file with the `@intrinsic` declarations of its framework decorators.
#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub source: String,
    pub declaration: Option<String>,
}

/// The modules shipped with this compiler version: name, source and
/// `@intrinsic` declarations.
const MODULES: &[(&str, &str, Option<&str>)] = &[
    ("alert", include_str!("../std/alert.wowlua"), None),
    (
        "aura",
        include_str!("../std/aura.wowlua"),
        Some(include_str!("../std/aura.d.wowlua")),
    ),
    (
        "bindings",
        include_str!("../std/bindings.wowlua"),
        Some(include_str!("../std/bindings.d.wowlua")),
    ),
    ("callbacks", include_str!("../std/callbacks.wowlua"), None),
    ("chat", include_str!("../std/chat.wowlua"), None),
    (
        "collections",
        include_str!("../std/collections.wowlua"),
        None,
    ),
    (
        "color",
        include_str!("../std/color.wowlua"),
        Some(include_str!("../std/color.d.wowlua")),
    ),
    (
        "comm",
        include_str!("../std/comm.wowlua"),
        Some(include_str!("../std/comm.d.wowlua")),
    ),
    (
        "combat",
        include_str!("../std/combat.wowlua"),
        Some(include_str!("../std/combat.d.wowlua")),
    ),
    (
        "debug",
        include_str!("../std/debug.wowlua"),
        Some(include_str!("../std/debug.d.wowlua")),
    ),
    ("dialog", include_str!("../std/dialog.wowlua"), None),
    ("errors", include_str!("../std/errors.wowlua"), None),
    ("format", include_str!("../std/format.wowlua"), None),
    (
        "frames",
        include_str!("../std/frames.wowlua"),
        Some(include_str!("../std/frames.d.wowlua")),
    ),
    (
        "events",
        include_str!("../std/events.wowlua"),
        Some(include_str!("../std/events.d.wowlua")),
    ),
    (
        "hooks",
        include_str!("../std/hooks.wowlua"),
        Some(include_str!("../std/hooks.d.wowlua")),
    ),
    ("item", include_str!("../std/item.wowlua"), None),
    ("links", include_str!("../std/links.wowlua"), None),
    ("list", include_str!("../std/list.wowlua"), None),
    (
        "jobs",
        include_str!("../std/jobs.wowlua"),
        Some(include_str!("../std/jobs.d.wowlua")),
    ),
    (
        "launcher",
        include_str!("../std/launcher.wowlua"),
        Some(include_str!("../std/launcher.d.wowlua")),
    ),
    (
        "lifecycle",
        include_str!("../std/lifecycle.wowlua"),
        Some(include_str!("../std/lifecycle.d.wowlua")),
    ),
    (
        "locale",
        include_str!("../std/locale.wowlua"),
        Some(include_str!("../std/locale.d.wowlua")),
    ),
    (
        "log",
        include_str!("../std/log.wowlua"),
        Some(include_str!("../std/log.d.wowlua")),
    ),
    (
        "media",
        include_str!("../std/media.wowlua"),
        Some(include_str!("../std/media.d.wowlua")),
    ),
    ("menu", include_str!("../std/menu.wowlua"), None),
    (
        "mock",
        include_str!("../std/mock.wowlua"),
        Some(include_str!("../std/mock.d.wowlua")),
    ),
    (
        "pool",
        include_str!("../std/pool.wowlua"),
        Some(include_str!("../std/pool.d.wowlua")),
    ),
    (
        "reflect",
        include_str!("../std/reflect.wowlua"),
        Some(include_str!("../std/reflect.d.wowlua")),
    ),
    (
        "saved",
        include_str!("../std/saved.wowlua"),
        Some(include_str!("../std/saved.d.wowlua")),
    ),
    ("secret", include_str!("../std/secret.wowlua"), None),
    (
        "secure",
        include_str!("../std/secure.wowlua"),
        Some(include_str!("../std/secure.d.wowlua")),
    ),
    ("serialize", include_str!("../std/serialize.wowlua"), None),
    ("spell", include_str!("../std/spell.wowlua"), None),
    (
        "settings",
        include_str!("../std/settings.wowlua"),
        Some(include_str!("../std/settings.d.wowlua")),
    ),
    (
        "slash",
        include_str!("../std/slash.wowlua"),
        Some(include_str!("../std/slash.d.wowlua")),
    ),
    ("table", include_str!("../std/table.wowlua"), None),
    (
        "test",
        include_str!("../std/test.wowlua"),
        Some(include_str!("../std/test.d.wowlua")),
    ),
    (
        "timer",
        include_str!("../std/timer.wowlua"),
        Some(include_str!("../std/timer.d.wowlua")),
    ),
    (
        "tooltip",
        include_str!("../std/tooltip.wowlua"),
        Some(include_str!("../std/tooltip.d.wowlua")),
    ),
    ("unit", include_str!("../std/unit.wowlua"), None),
    ("utf8", include_str!("../std/utf8.wowlua"), None),
    ("version", include_str!("../std/version.wowlua"), None),
];

/// The names of the modules shipped with this compiler version (`events`
/// for `wowlua:events`).
pub fn module_names() -> Vec<&'static str> {
    MODULES.iter().map(|(n, _, _)| *n).collect()
}

/// A standard-library module by name (`events` for `wowlua:events`).
///
/// `WOWLUA_STDLIB_DIR` points the compiler at a directory of modules
/// (`<name>.wowlua`, `<name>.d.wowlua`) instead, for developing and testing
/// the standard library.
pub fn module(name: &str) -> Option<Module> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return None;
    }
    if let Some(dir) = std::env::var_os("WOWLUA_STDLIB_DIR") {
        let dir = std::path::PathBuf::from(dir);
        let source = std::fs::read_to_string(dir.join(format!("{name}.wowlua"))).ok()?;
        let declaration = std::fs::read_to_string(dir.join(format!("{name}.d.wowlua"))).ok();
        return Some(Module {
            name: name.to_string(),
            source,
            declaration,
        });
    }
    MODULES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(n, source, declaration)| Module {
            name: n.to_string(),
            source: source.to_string(),
            declaration: declaration.map(str::to_string),
        })
}

/// The source path of a standard-library module inside a build
/// (`@wowlua/events.wowlua`); its module ID is `wowlua:events`.
pub fn src_rel(name: &str) -> String {
    format!("@wowlua/{name}.wowlua")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_names_are_checked() {
        assert!(module("../etc").is_none());
        assert!(module("").is_none());
        assert_eq!(src_rel("events"), "@wowlua/events.wowlua");
        assert!(declarations()[0].text.contains("declare function print"));
    }
}
