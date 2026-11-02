//! A terrain tile (decision T522): a map's ADT, 533⅓ yards a side in
//! 16 × 16 chunks, read from its root file (each chunk's heights, normals
//! and holes) and its textures file (`_tex0`: the textures, each chunk's
//! layers and their blends), and its liquids (`MH2O`, decision T557).
//!
//! Places are the world's: north and west yards from the map's middle. A
//! tile's north-west corner is at `(32 - row, 32 - column)` tiles; a
//! chunk's 145 heights are a 9 × 9 grid of corners and the 8 × 8 middles
//! between them, rows interleaved (9, 8, 9, …).

/// A tile's side (yards).
pub const TILE: f64 = 1600.0 / 3.0;
/// A chunk's side: a sixteenth of a tile.
pub const CHUNK: f64 = TILE / 16.0;
/// The grid's step: an eighth of a chunk.
pub const UNIT: f64 = CHUNK / 8.0;

/// A tile: its 256 chunks (`chunks[y * 16 + x]`, from the north-west) and
/// the textures its layers name (file IDs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile {
    pub chunks: Vec<Chunk>,
    pub textures: Vec<u32>,
    /// Its water, lava and slime, chunk by chunk.
    pub liquids: Vec<Liquid>,
}

/// A body of liquid over part of a chunk (`MH2O`'s instance): over the
/// chunk's 8 × 8 squares from `x`, `y` (west to east, north to south),
/// `width` by `height` of them, those `exists` names.
#[derive(Clone, Debug, PartialEq)]
pub struct Liquid {
    /// The chunk's column and row in the tile.
    pub chunk: (u32, u32),
    /// `LiquidType`.
    pub kind: u32,
    /// `LiquidObject` (42 and up), else the vertex format itself.
    pub object: u32,
    pub min: f32,
    pub max: f32,
    pub x: u8,
    pub y: u8,
    pub width: u8,
    pub height: u8,
    /// Each square of the `width` × `height`, row by row.
    pub exists: Vec<bool>,
    /// The vertices' data as stored, `(width + 1) × (height + 1)` of them;
    /// read by its vertex format ([`Liquid::heights`]).
    pub data: Vec<u8>,
}

impl Liquid {
    /// The vertices' heights and depths (0 to 255) by the vertex format:
    /// 0 heights then depths, 1 heights then texture coordinates, 2 depths
    /// alone (flat at `min`), 3 heights, coordinates, depths. A height
    /// outside the liquid's own `min` to `max` is a format misjudged: its
    /// level (`min`) instead.
    pub fn heights(&self, format: u32) -> (Vec<f32>, Vec<u8>) {
        let (mut heights, depths) = self.stored(format);
        for h in &mut heights {
            if !h.is_finite() || *h < self.min - 0.5 || *h > self.max + 0.5 {
                *h = self.min;
            }
        }
        (heights, depths)
    }

    fn stored(&self, format: u32) -> (Vec<f32>, Vec<u8>) {
        let n = (usize::from(self.width) + 1) * (usize::from(self.height) + 1);
        // (none stored: flat at its level, deep)
        if self.data.is_empty() {
            return (vec![self.min; n], vec![255; n]);
        }
        let height = |i: usize| f32_at(&self.data, i * 4);
        let byte = |at: usize| self.data.get(at).copied().unwrap_or(255);
        match format {
            0 => (
                (0..n).map(height).collect(),
                (0..n).map(|i| byte(n * 4 + i)).collect(),
            ),
            1 => ((0..n).map(height).collect(), vec![255; n]),
            3 => (
                (0..n).map(height).collect(),
                (0..n).map(|i| byte(n * 8 + i)).collect(),
            ),
            _ => (vec![self.min; n], (0..n).map(byte).collect()),
        }
    }
}

/// A chunk: its heights over `base`, its normals, its holes and its
/// texture layers.
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub x: u32,
    pub y: u32,
    pub base: f32,
    /// 145 heights over `base`: rows of 9 corners and 8 middles.
    pub heights: Vec<f32>,
    /// 145 normals, each axis a byte of 127ths (north, west, up).
    pub normals: Vec<[i8; 3]>,
    /// The 8 × 8 squares with no ground: bit `row * 8 + column`.
    pub holes: u64,
    pub area: u32,
    pub layers: Vec<Layer>,
    /// Which layer's ground effect grows in each of its 8 × 8 squares
    /// (`row * 8 + column`; decision T560).
    pub effects: [u8; 64],
    /// The squares that grow none: bit `row * 8 + column`.
    pub no_effect: u64,
    /// Its baked shadow (`MCSH`, decision T563): 64 rows from the north,
    /// each a bit a column from the west, set where shadowed; none where
    /// the build has none (WoW Forever's).
    pub shadow: Option<Vec<u64>>,
    /// Its header's flags (`0x8000`: its maps whole, their last row and
    /// column not copied from the one before).
    pub flags: u32,
}

/// A texture layer: its texture (an index into the tile's), and how much
/// of it shows where (64 × 64, from the chunk's north-west; the first
/// layer has none: all of it).
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub texture: u32,
    pub alpha: Option<Vec<u8>>,
    /// Its ground effect (`GroundEffectTexture`), 0 none (T560).
    pub effect: u32,
}

/// The chunks of a file: each one's tag and body.
fn chunks(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut o = 0;
    while o + 8 <= data.len() {
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&data[o..o + 4]);
        // (stored reversed)
        tag.reverse();
        let size = u32::from_le_bytes(data[o + 4..o + 8].try_into().unwrap()) as usize;
        let end = (o + 8 + size).min(data.len());
        out.push((tag, &data[o + 8..end]));
        o = end;
    }
    out
}

/// A chunk's own subchunks, after its header (`MCNK`'s): `header` bytes
/// of it. A normals subchunk (`MCNR`) of 435 bytes has 13 more after it
/// its size leaves out.
fn subchunks(body: &[u8], header: usize) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut o = header;
    while o + 8 <= body.len() {
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&body[o..o + 4]);
        tag.reverse();
        let size = u32::from_le_bytes(body[o + 4..o + 8].try_into().unwrap()) as usize;
        let end = (o + 8 + size).min(body.len());
        out.push((tag, &body[o + 8..end]));
        o = end;
        if &tag == b"MCNR" && size == 435 && !body.get(o..o + 4).is_some_and(is_tag) {
            o += 13;
        }
    }
    out
}

fn is_tag(b: &[u8]) -> bool {
    b.iter()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4)
        .map_or(0, |x| u32::from_le_bytes(x.try_into().unwrap()))
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_bits(u32_at(b, at))
}

/// A tile from its root file and its textures file (`big_alpha`: the
/// map's blends are 8 bits a texel, its WDT's flag); none for a root file
/// without chunks.
pub fn read(root: &[u8], tex: Option<&[u8]>, big_alpha: bool) -> Option<Tile> {
    let mut tile = Tile::default();
    for (tag, body) in chunks(root) {
        if &tag == b"MH2O" {
            tile.liquids = liquids(body);
            continue;
        }
        if &tag != b"MCNK" || body.len() < 128 {
            continue;
        }
        let flags = u32_at(body, 0);
        let holes = if flags & 0x10000 != 0 {
            u64::from_le_bytes(body[20..28].try_into().unwrap())
        } else {
            low_res_holes(u16::from_le_bytes(body[60..62].try_into().unwrap()))
        };
        let mut chunk = Chunk {
            x: u32_at(body, 4),
            y: u32_at(body, 8),
            base: f32_at(body, 112),
            heights: vec![0.0; 145],
            normals: vec![[0, 0, 127]; 145],
            holes,
            area: u32_at(body, 52),
            layers: Vec::new(),
            // (2 bits a square, from the low bits; then a bit a square)
            effects: std::array::from_fn(|i| (body[64 + i / 4] >> ((i % 4) * 2)) & 3),
            no_effect: u64::from_le_bytes(body[80..88].try_into().unwrap()),
            shadow: None,
            flags,
        };
        for (sub, b) in subchunks(body, 128) {
            match &sub {
                b"MCVT" => {
                    for (i, h) in chunk.heights.iter_mut().enumerate() {
                        *h = f32_at(b, i * 4);
                    }
                }
                b"MCNR" => {
                    for (i, n) in chunk.normals.iter_mut().enumerate() {
                        if let Some(v) = b.get(i * 3..i * 3 + 3) {
                            *n = [v[0] as i8, v[1] as i8, v[2] as i8];
                        }
                    }
                }
                _ => {}
            }
        }
        tile.chunks.push(chunk);
    }
    if tile.chunks.is_empty() {
        return None;
    }
    tile.chunks.sort_by_key(|c| (c.y, c.x));
    if let Some(tex) = tex {
        read_textures(&mut tile, tex, big_alpha);
    }
    Some(tile)
}

/// `MH2O`: for each of the 256 chunks (by index, `y * 16 + x`) its
/// instances; offsets from the chunk's data.
fn liquids(body: &[u8]) -> Vec<Liquid> {
    let mut out = Vec::new();
    for i in 0..256usize {
        let at = i * 12;
        let (first, count) = (u32_at(body, at) as usize, u32_at(body, at + 4) as usize);
        if first == 0 || count == 0 {
            continue;
        }
        for k in 0..count.min(16) {
            let o = first + k * 24;
            if o + 24 > body.len() {
                break;
            }
            let half = |a: usize| u16::from_le_bytes([body[a], body[a + 1]]);
            let (x, y, width, height) = (body[o + 12], body[o + 13], body[o + 14], body[o + 15]);
            if width == 0 || height == 0 || x + width > 8 || y + height > 8 {
                continue;
            }
            let squares = usize::from(width) * usize::from(height);
            let bits = u32_at(body, o + 16) as usize;
            let exists = (0..squares)
                .map(|s| {
                    bits == 0
                        || body
                            .get(bits + s / 8)
                            .is_some_and(|b| b & (1 << (s % 8)) != 0)
                })
                .collect();
            let verts = (usize::from(width) + 1) * (usize::from(height) + 1);
            let data_at = u32_at(body, o + 20) as usize;
            let data = if data_at == 0 {
                Vec::new()
            } else {
                body.get(data_at..(data_at + verts * 9).min(body.len()))
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default()
            };
            out.push(Liquid {
                chunk: ((i % 16) as u32, (i / 16) as u32),
                kind: u32::from(half(o)),
                object: u32::from(half(o + 2)),
                min: f32_at(body, o + 4),
                max: f32_at(body, o + 8),
                x,
                y,
                width,
                height,
                exists,
                data,
            });
        }
    }
    out
}

/// The old 4 × 4 holes (a bit a 2 × 2 of squares) as 8 × 8.
fn low_res_holes(bits: u16) -> u64 {
    let mut out = 0u64;
    for row in 0..8 {
        for col in 0..8 {
            if bits & (1 << ((row / 2) * 4 + col / 2)) != 0 {
                out |= 1 << (row * 8 + col);
            }
        }
    }
    out
}

/// The textures file: the textures (`MDID`, file IDs), each chunk's layers
/// (`MCLY`) and blends (`MCAL`), in the root's order.
fn read_textures(tile: &mut Tile, tex: &[u8], big_alpha: bool) {
    let mut n = 0;
    for (tag, body) in chunks(tex) {
        match &tag {
            b"MDID" => {
                tile.textures = (0..body.len() / 4).map(|i| u32_at(body, i * 4)).collect();
            }
            b"MCNK" => {
                let Some(chunk) = tile.chunks.get_mut(n) else {
                    break;
                };
                n += 1;
                let subs = subchunks(body, 0);
                let mcly = subs.iter().find(|(t, _)| t == b"MCLY").map(|(_, b)| *b);
                let mcal = subs.iter().find(|(t, _)| t == b"MCAL").map(|(_, b)| *b);
                if let Some((_, b)) = subs.iter().find(|(t, _)| t == b"MCSH")
                    && b.len() >= 512
                {
                    chunk.shadow = Some(shadow_map(b, chunk.flags & 0x8000 == 0));
                }
                let Some(mcly) = mcly else {
                    continue;
                };
                for i in 0..mcly.len() / 16 {
                    let at = i * 16;
                    let texture = u32_at(mcly, at);
                    let flags = u32_at(mcly, at + 4);
                    let offset = u32_at(mcly, at + 8) as usize;
                    let alpha = (i > 0 && flags & 0x100 != 0)
                        .then(|| mcal.and_then(|a| a.get(offset..)))
                        .flatten()
                        .map(|a| alpha_map(a, flags & 0x200 != 0, big_alpha));
                    chunk.layers.push(Layer {
                        texture,
                        alpha,
                        effect: u32_at(mcly, at + 12),
                    });
                }
            }
            _ => {}
        }
    }
}

/// A baked shadow's 64 rows of 64 bits; the last row and column copied
/// from the ones before where the chunk's maps are not whole (`fix`).
fn shadow_map(b: &[u8], fix: bool) -> Vec<u64> {
    let mut rows: Vec<u64> = (0..64)
        .map(|r| u64::from_le_bytes(b[r * 8..r * 8 + 8].try_into().unwrap()))
        .collect();
    if fix {
        for row in &mut rows {
            *row = (*row & !(1 << 63)) | ((*row >> 62 & 1) << 63);
        }
        rows[63] = rows[62];
    }
    rows
}

/// A layer's blend, 64 × 64 bytes: run-length coded (`compressed`), 8 bits
/// a texel (`big`), else 4.
fn alpha_map(a: &[u8], compressed: bool, big: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(4096);
    if compressed {
        let mut i = 0;
        while out.len() < 4096 && i < a.len() {
            let head = a[i];
            i += 1;
            let count = (head & 0x7F) as usize;
            if head & 0x80 != 0 {
                let v = a.get(i).copied().unwrap_or(0);
                i += 1;
                out.extend(std::iter::repeat_n(v, count));
            } else {
                let end = (i + count).min(a.len());
                out.extend_from_slice(&a[i..end]);
                i = end;
            }
        }
    } else if big {
        out.extend_from_slice(&a[..a.len().min(4096)]);
    } else {
        for &b in a.iter().take(2048) {
            out.push((b & 0x0F) * 17);
            out.push((b >> 4) * 17);
        }
    }
    out.resize(4096, 0);
    out
}

/// Which tile holds a place: (column, row).
pub fn tile_of(north: f64, west: f64) -> (i32, i32) {
    (
        (32.0 - west / TILE).floor() as i32,
        (32.0 - north / TILE).floor() as i32,
    )
}

/// A tile's north-west corner: (north, west).
pub fn corner(column: i32, row: i32) -> (f64, f64) {
    (
        (32.0 - f64::from(row)) * TILE,
        (32.0 - f64::from(column)) * TILE,
    )
}

impl Chunk {
    /// The height of its grid point: a corner (row and column 0 to 8) or,
    /// `middle`, the middle south-east of it (0 to 7).
    pub fn height(&self, row: usize, col: usize, middle: bool) -> f32 {
        let i = row * 17 + if middle { 9 + col } else { col };
        self.base + self.heights[i]
    }

    /// Whether its square (row, column, 0 to 7) is a hole.
    pub fn hole(&self, row: usize, col: usize) -> bool {
        self.holes & (1 << (row * 8 + col)) != 0
    }
}

impl Tile {
    /// The ground's height at a place `south` and `east` yards from the
    /// tile's north-west corner, none over a hole or off the tile: the
    /// grid's triangles (each square four, around its middle).
    pub fn height_at(&self, south: f64, east: f64) -> Option<f64> {
        if !(0.0..TILE).contains(&south) || !(0.0..TILE).contains(&east) {
            return None;
        }
        let (cy, cx) = ((south / CHUNK) as usize, (east / CHUNK) as usize);
        let chunk = self
            .chunks
            .iter()
            .find(|c| c.x as usize == cx && c.y as usize == cy)?;
        let (s, e) = (
            (south - cy as f64 * CHUNK) / UNIT,
            (east - cx as f64 * CHUNK) / UNIT,
        );
        let (row, col) = ((s as usize).min(7), (e as usize).min(7));
        if chunk.hole(row, col) {
            return None;
        }
        let (fs, fe) = (s - row as f64, e - col as f64);
        let h = |r: usize, c: usize| f64::from(chunk.height(r, c, false));
        let mid = f64::from(chunk.height(row, col, true));
        let (nw, ne, sw, se) = (
            h(row, col),
            h(row, col + 1),
            h(row + 1, col),
            h(row + 1, col + 1),
        );
        // The triangle around the middle the place is in: its two corners
        // and the middle, weighed.
        let (dx, dy) = (fe - 0.5, fs - 0.5);
        let (a, b, ta, tb) = if dy.abs() >= dx.abs() {
            if dy < 0.0 {
                (nw, ne, -dy, dx)
            } else {
                (sw, se, dy, dx)
            }
        } else if dx < 0.0 {
            (nw, sw, -dx, dy)
        } else {
            (ne, se, dx, dy)
        };
        // (`ta`: how far from the middle toward the edge, 0 to 0.5; `tb`:
        // along the edge, -0.5 to 0.5)
        let edge = a + (b - a) * (tb / (2.0 * ta.max(1e-9)) + 0.5).clamp(0.0, 1.0);
        Some(mid + (edge - mid) * (ta * 2.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut t = *tag;
        t.reverse();
        let mut v = t.to_vec();
        v.extend((body.len() as u32).to_le_bytes());
        v.extend(body);
        v
    }

    /// A tile of one chunk (0, 0) at height 100, its heights rising 1 a
    /// grid row southward, the square (2, 3) a hole.
    fn one_chunk() -> Vec<u8> {
        let mut header = vec![0u8; 128];
        header[0..4].copy_from_slice(&0x10000u32.to_le_bytes());
        let holes: u64 = 1 << (2 * 8 + 3);
        header[20..28].copy_from_slice(&holes.to_le_bytes());
        header[112..116].copy_from_slice(&100f32.to_le_bytes());
        let mut mcvt = Vec::new();
        for j in 0..17 {
            let (n, row) = if j % 2 == 0 {
                (9, j as f32 / 2.0)
            } else {
                (8, j as f32 / 2.0)
            };
            for _ in 0..n {
                mcvt.extend(row.to_le_bytes());
            }
        }
        let mut body = header;
        body.extend(chunk(b"MCVT", &mcvt));
        body.extend(chunk(b"MCNR", &[0u8; 435]));
        body.extend([0u8; 13]);
        body.extend(chunk(b"MCLV", &[0u8; 4]));
        let mut file = chunk(b"MVER", &18u32.to_le_bytes());
        file.extend(chunk(b"MCNK", &body));
        file
    }

    #[test]
    fn a_tiles_heights_holes_and_padded_normals() {
        let tile = read(&one_chunk(), None, true).unwrap();
        assert_eq!(tile.chunks.len(), 1);
        let c = &tile.chunks[0];
        assert_eq!(c.height(0, 0, false), 100.0);
        assert_eq!(c.height(8, 8, false), 108.0);
        assert_eq!(c.height(0, 0, true), 100.5);
        assert!(c.hole(2, 3) && !c.hole(3, 2));
        // (a slope: the height follows the south, between the corners and
        // through the middles)
        let at = |s: f64, e: f64| tile.height_at(s * UNIT, e * UNIT).unwrap();
        assert!((at(0.0, 0.0) - 100.0).abs() < 1e-9);
        assert!((at(0.5, 0.5) - 100.5).abs() < 1e-9);
        assert!((at(1.25, 4.75) - 101.25).abs() < 1e-9);
        assert_eq!(tile.height_at(2.5 * UNIT, 3.5 * UNIT), None);
        assert_eq!(tile.height_at(-1.0, 0.0), None);
    }

    #[test]
    fn blends_of_each_kind() {
        let run = alpha_map(&[0x80 | 3, 9, 2, 1, 2], true, true);
        assert_eq!(&run[..5], &[9, 9, 9, 1, 2]);
        let half = alpha_map(&[0xF1; 2048], false, false);
        assert_eq!(&half[..2], &[17, 255]);
        assert_eq!(low_res_holes(1), 0b11 | 0b11 << 8);
    }

    #[test]
    fn a_chunk_s_baked_shadow() {
        let mut mcsh = vec![0u8; 512];
        // (row 0: columns 0 and 62; row 62: column 1)
        mcsh[0] = 1;
        mcsh[7] = 0x40;
        mcsh[62 * 8] = 2;
        let mut body = vec![0u8; 0];
        body.extend(chunk(b"MCLY", &[0; 16]));
        body.extend(chunk(b"MCSH", &mcsh));
        let tex = chunk(b"MCNK", &body);
        let tile = read(&one_chunk(), Some(&tex), true).unwrap();
        let rows = tile.chunks[0].shadow.as_ref().unwrap();
        // (the maps not whole: the last column and row copied)
        assert_eq!(rows[0], 1 | 1 << 62 | 1 << 63);
        assert_eq!(rows[63], 2);
        let whole = shadow_map(&mcsh, false);
        assert_eq!(whole[0], 1 | 1 << 62);
        assert_eq!(whole[63], 0);
        assert_eq!(
            read(&one_chunk(), None, true).unwrap().chunks[0].shadow,
            None
        );
    }

    #[test]
    fn places_and_tiles() {
        // (the middle of the map: tile 32, 32's north-west corner)
        assert_eq!(tile_of(0.0, 0.0), (32, 32));
        assert_eq!(tile_of(-1.0, -1.0), (32, 32));
        assert_eq!(tile_of(1.0, 1.0), (31, 31));
        assert_eq!(corner(32, 32), (0.0, 0.0));
    }
}
