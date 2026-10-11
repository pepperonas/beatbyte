//! The `.sng` package: one file holding a chart folder.
//!
//! Layout (little-endian): `SNGPKG`, a `u32` version, a 16-byte XOR
//! mask, then a metadata block (`u64` length, `u64` count, then per pair
//! an `i32`-prefixed key and value — the song's `song.ini`), then a file
//! index (`u64` length, `u64` count, then per file a `u8`-prefixed name,
//! a `u64` size and a `u64` offset), then the file bytes. Byte `i` of a
//! file is XORed with `mask[i % 16] ^ (i & 0xFF)`.
//!
//! **Untrusted input**, all of it: every length is checked against what
//! is left before it is used, the whole package is capped at
//! [`MAX_PACKAGE`], and a file name is accepted only as a plain name
//! with a known extension — no path, no drive letter, no `..` — because
//! it becomes a file on the player's disk.

use std::path::Path;

/// The largest package this reads (a chart with stems is tens of MB;
/// the cap leaves room for a long song in a lossless format).
pub const MAX_PACKAGE: usize = 512 * 1024 * 1024;

/// The most metadata pairs or files a package may list.
const MAX_ENTRIES: u64 = 4096;

/// The extensions a chart folder may hold; everything else (videos,
/// images we do not show, unknown files) is skipped.
pub const KEPT_EXTENSIONS: [&str; 7] = ["chart", "mid", "ini", "ogg", "opus", "mp3", "wav"];

/// A decoded package: its metadata and its kept files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Package {
    /// `song.ini`'s pairs, in package order.
    pub meta: Vec<(String, String)>,
    /// The files worth writing: (plain name, bytes).
    pub files: Vec<(String, Vec<u8>)>,
}

impl Package {
    /// Whether it carries a chart (`notes.chart` or `notes.mid`).
    #[must_use]
    pub fn has_chart(&self) -> bool {
        self.files
            .iter()
            .any(|(name, _)| matches!(name.to_lowercase().as_str(), "notes.chart" | "notes.mid"))
    }

    /// Write the package as a chart folder into `dir` (which must
    /// exist): every kept file, and `song.ini` from the metadata, plus
    /// `beatbyte_game_rip = <game>` when the catalogue said it is a rip
    /// — the download itself does not say, and the converter files a rip
    /// as a `GR` version by that line.
    ///
    /// # Errors
    /// When a file cannot be written.
    pub fn write_folder(&self, dir: &Path, game_rip: Option<&str>) -> Result<(), String> {
        for (name, bytes) in &self.files {
            if name.eq_ignore_ascii_case("song.ini") {
                continue; // written from the metadata below
            }
            std::fs::write(dir.join(name), bytes)
                .map_err(|e| format!("cannot write {name}: {e}"))?;
        }
        let mut ini = String::from("[song]\n");
        for (key, value) in &self.meta {
            let clean = |s: &str| s.replace(['\n', '\r'], " ");
            ini.push_str(&format!("{} = {}\n", clean(key), clean(value)));
        }
        if let Some(game) = game_rip {
            ini.push_str(&format!(
                "beatbyte_game_rip = {}\n",
                game.replace(['\n', '\r'], " ")
            ));
        }
        std::fs::write(dir.join("song.ini"), ini).map_err(|e| format!("cannot write song.ini: {e}"))
    }
}

/// Whether a name from the package may become a file: a plain name, no
/// separator, no drive, no hidden or parent entry, a kept extension.
/// Pure — tested.
#[must_use]
pub fn safe_name(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
        && name.len() <= 128
        && KEPT_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// A cursor over the package bytes that refuses to read past the end.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|end| *end <= self.data.len())
            .ok_or("the package ends early")?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u64(&mut self) -> Result<u64, String> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| "short read")?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn i32_len(&mut self) -> Result<usize, String> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| "short read")?;
        usize::try_from(i32::from_le_bytes(bytes)).map_err(|_| "a negative length".to_owned())
    }

    fn string(&mut self, len: usize) -> Result<String, String> {
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }
}

/// Decode a package. Files with an unsafe name or an extension we do not
/// keep are skipped, not refused; a package without a chart is refused.
///
/// # Errors
/// When the bytes are not a package, are cut short, lie about a length,
/// are larger than [`MAX_PACKAGE`], or hold no chart.
pub fn decode(data: &[u8]) -> Result<Package, String> {
    if data.len() > MAX_PACKAGE {
        return Err("the package is larger than any chart".to_owned());
    }
    let mut r = Reader { data, pos: 0 };
    if r.take(6)? != b"SNGPKG" {
        return Err("not a chart package".to_owned());
    }
    r.take(4)?; // version
    let mask: [u8; 16] = r.take(16)?.try_into().map_err(|_| "short mask")?;
    r.u64()?; // metadata block length
    let pairs = r.u64()?;
    if pairs > MAX_ENTRIES {
        return Err("the package lists too much metadata".to_owned());
    }
    let mut meta = Vec::new();
    for _ in 0..pairs {
        let key_len = r.i32_len()?;
        let key = r.string(key_len)?;
        let value_len = r.i32_len()?;
        let value = r.string(value_len)?;
        meta.push((key, value));
    }
    r.u64()?; // file index length
    let count = r.u64()?;
    if count > MAX_ENTRIES {
        return Err("the package lists too many files".to_owned());
    }
    let mut index = Vec::new();
    for _ in 0..count {
        let name_len = usize::from(r.u8()?);
        let name = r.string(name_len)?;
        let size = usize::try_from(r.u64()?).map_err(|_| "a file larger than memory")?;
        let offset = usize::try_from(r.u64()?).map_err(|_| "an offset past memory")?;
        index.push((name, size, offset));
    }
    let mut files = Vec::new();
    for (name, size, offset) in index {
        let end = offset
            .checked_add(size)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| format!("{name} reaches past the end of the package"))?;
        if !safe_name(&name) {
            continue;
        }
        let bytes = data[offset..end]
            .iter()
            .enumerate()
            .map(|(i, b)| {
                #[allow(clippy::cast_possible_truncation)] // the low byte, on purpose
                let low = i as u8;
                b ^ mask[i % 16] ^ low
            })
            .collect();
        files.push((name, bytes));
    }
    let package = Package { meta, files };
    if package.has_chart() {
        Ok(package)
    } else {
        Err("the package holds no chart".to_owned())
    }
}

/// Encode a package — the inverse of [`decode`], for tests and tools.
#[must_use]
pub fn encode(meta: &[(&str, &str)], files: &[(&str, &[u8])], mask: [u8; 16]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"SNGPKG");
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&mask);
    let mut block = Vec::new();
    for (key, value) in meta {
        for part in [key, value] {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)] // test sizes
            block.extend_from_slice(&(part.len() as i32).to_le_bytes());
            block.extend_from_slice(part.as_bytes());
        }
    }
    out.extend_from_slice(&(block.len() as u64).to_le_bytes());
    out.extend_from_slice(&(meta.len() as u64).to_le_bytes());
    out.extend_from_slice(&block);
    // Index first, with offsets computed from where the data will start.
    let index_len: usize = files.iter().map(|(n, _)| 1 + n.len() + 16).sum();
    let mut offset = out.len() + 16 + index_len;
    let mut index = Vec::new();
    for (name, bytes) in files {
        #[allow(clippy::cast_possible_truncation)] // test names are short
        index.push(name.len() as u8);
        index.extend_from_slice(name.as_bytes());
        index.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        index.extend_from_slice(&(offset as u64).to_le_bytes());
        offset += bytes.len();
    }
    out.extend_from_slice(&(index.len() as u64).to_le_bytes());
    out.extend_from_slice(&(files.len() as u64).to_le_bytes());
    out.extend_from_slice(&index);
    for (_, bytes) in files {
        out.extend(bytes.iter().enumerate().map(|(i, b)| {
            #[allow(clippy::cast_possible_truncation)]
            let low = i as u8;
            b ^ mask[i % 16] ^ low
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASK: [u8; 16] = [7, 1, 200, 33, 9, 250, 0, 12, 99, 5, 128, 64, 17, 3, 77, 241];

    /// A package round-trips: metadata in order, files byte for byte,
    /// across more than one turn of the 256-byte low-byte cycle.
    #[test]
    fn a_package_round_trips() {
        let audio: Vec<u8> = (0..5000u32).map(|i| (i * 31 % 251) as u8).collect();
        let bytes = encode(
            &[("name", "Kryptonite"), ("artist", "3 Doors Down")],
            &[("notes.chart", b"[Song]\n{\n}\n"), ("song.ogg", &audio)],
            MASK,
        );
        let package = decode(&bytes).expect("decodes");
        assert_eq!(
            package.meta,
            vec![
                ("name".to_owned(), "Kryptonite".to_owned()),
                ("artist".to_owned(), "3 Doors Down".to_owned()),
            ]
        );
        assert_eq!(package.files[0].1, b"[Song]\n{\n}\n");
        assert_eq!(package.files[1].1, audio);
    }

    /// A name that would leave the folder, or a file we do not keep, is
    /// skipped — it never becomes a file.
    #[test]
    fn an_unsafe_or_unknown_file_is_skipped() {
        let bytes = encode(
            &[],
            &[
                ("notes.mid", b"MThd"),
                ("../../evil.ogg", b"x"),
                ("C:evil.ogg", b"x"),
                ("dir/song.ogg", b"x"),
                (".hidden.ogg", b"x"),
                ("video.mp4", b"x"),
                ("guitar.ogg", b"ok"),
            ],
            MASK,
        );
        let names: Vec<String> = decode(&bytes)
            .expect("decodes")
            .files
            .into_iter()
            .map(|(n, _)| n)
            .collect();
        assert_eq!(names, vec!["notes.mid", "guitar.ogg"]);
        for bad in [
            "..", "a/b.ogg", "a\\b.ogg", "x:y.ogg", "song", "song.exe", ".ogg",
        ] {
            assert!(!safe_name(bad), "{bad}");
        }
        for good in ["notes.chart", "Song.OGG", "drums_1.opus", "song.ini"] {
            assert!(safe_name(good), "{good}");
        }
    }

    /// Lies and truncation are errors, never a panic or an allocation
    /// of whatever size the file claims.
    #[test]
    fn a_lying_or_cut_package_is_refused() {
        assert!(decode(b"").is_err());
        assert!(decode(b"NOTPKG0000000000000000000000").is_err());
        let good = encode(&[("a", "b")], &[("notes.chart", b"x")], MASK);
        for cut in [10, 30, 40, good.len() - 1] {
            assert!(decode(&good[..cut]).is_err(), "cut at {cut}");
        }
        // A file whose size runs past the end.
        let mut lying = good.clone();
        let size_at = lying.len() - 1 - 16; // the last file's size field
        lying[size_at..size_at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode(&lying).is_err());
        // Without a chart there is nothing to import.
        let no_chart = encode(&[], &[("song.ogg", b"x")], MASK);
        assert_eq!(
            decode(&no_chart),
            Err("the package holds no chart".to_owned())
        );
    }

    /// The folder holds the files and a song.ini built from the
    /// metadata — with the rip line when the catalogue said so, and no
    /// line break smuggled into a key or a value.
    #[test]
    fn a_package_writes_a_chart_folder() {
        let dir = std::env::temp_dir().join(format!("bb-sng-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let bytes = encode(
            &[("name", "Song\nartist = Evil")],
            &[("notes.chart", b"x"), ("song.ogg", b"y")],
            MASK,
        );
        let package = decode(&bytes).expect("decodes");
        package
            .write_folder(&dir, Some("Rock Band 2 DLC"))
            .expect("writes");
        let ini = std::fs::read_to_string(dir.join("song.ini")).expect("ini");
        assert!(ini.starts_with("[song]\n"));
        assert!(ini.contains("name = Song artist = Evil\n"), "{ini}");
        assert!(ini.contains("beatbyte_game_rip = Rock Band 2 DLC\n"));
        assert_eq!(std::fs::read(dir.join("song.ogg")).expect("ogg"), b"y");
        std::fs::remove_dir_all(&dir).ok();
    }
}
