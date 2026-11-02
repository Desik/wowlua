//! The user's own WoW installation as a file source (spec debugger §18.10,
//! decisions T210, T310): the local CASC storage read by path or file ID,
//! and the client's image formats decoded. Nothing read here is copied
//! anywhere but memory, except the game data `wowlua data pull` copies
//! into the user's own cache (decision T203, [`Casc::pull`]).
//!
//! The storage is reached through a [`Storage`] (the file system natively,
//! the browser's File System Access API there), so the crate has no file
//! access of its own (decision T212).

pub mod adt;
pub mod atlas;
pub mod blp;
mod blte;
mod casc;
pub mod character;
pub mod clutter;
pub mod collide;
pub mod db2;
pub mod display;
pub mod globalcolors;
pub mod globalstrings;
pub mod jenkins;
pub mod light;
pub mod liquid;
pub mod m2;
pub mod obj;
mod salsa20;
mod schema;
pub mod spells;
pub mod spelltext;
pub mod tables;
pub mod tga;
pub mod wdb;
pub mod wdt;
pub mod wmo;

pub use casc::{Casc, CascError};

/// Where the installation's files are read from: paths relative to the
/// installation (`Data/data/data.000`, `.build.info`).
pub trait Storage {
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// `len` bytes at `offset`.
    fn read_at(&self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>>;
    /// The names of the files in a folder.
    fn list(&self, dir: &str) -> Vec<String>;
}

/// An image, 8 bits per channel, rows top to bottom.
#[derive(Clone, Debug)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The storage of a folder on this machine.
#[cfg(not(target_arch = "wasm32"))]
pub struct FsStorage {
    pub root: std::path::PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl Storage for FsStorage {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.root.join(path)).ok()
    }

    fn read_at(&self, path: &str, offset: u64, len: usize) -> Option<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(self.root.join(path)).ok()?;
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut buf = vec![0; len];
        f.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    fn list(&self, dir: &str) -> Vec<String> {
        match std::fs::read_dir(self.root.join(dir)) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

/// Decodes an image file by its bytes: BLP or TGA.
pub fn decode_image(bytes: &[u8]) -> Option<Rgba> {
    if bytes.starts_with(b"BLP2") {
        blp::decode(bytes)
    } else {
        tga::decode(bytes)
    }
}
