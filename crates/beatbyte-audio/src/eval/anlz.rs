//! Reading beat grids out of Rekordbox's own analysis files.
//!
//! Rekordbox writes an `ANLZ0000.DAT` beside every analysed track.
//! Two of its sections are all a grid needs:
//!
//! - **`PPTH`** — the track's file path, UTF-16 big-endian. This is
//!   what identifies the grid without the library database.
//! - **`PQTZ`** — the beat grid: one 8-byte entry per beat, holding
//!   the beat's position in the bar (1–4, so downbeats come free),
//!   the tempo in hundredths of a BPM, and the time in milliseconds.
//!
//! The file format is not documented by Pioneer; it is publicly
//! reverse-engineered, so **nothing here is trusted on faith**. Every
//! parse is checked against the file's own redundancy: the beat count
//! stated in the header must agree with the count implied by the
//! section length, or the section is rejected. That check earned its
//! place immediately — the first version of this parser read the
//! count from the wrong offset and produced 524 288 beats at 8.49 BPM
//! from a file that really holds 849 beats at 124.42.
//!
//! ⚠️ `master.db` is deliberately untouched. Rekordbox 6 encrypts it,
//! and the path in `PPTH` makes it unnecessary.

use crate::eval::GroundTruth;

/// Bytes per beat entry in a `PQTZ` section.
const BEAT_ENTRY: usize = 8;

/// What one analysis file states about its track.
#[derive(Debug, Clone, PartialEq)]
pub struct AnlzTrack {
    /// The audio path as the file states it (root-relative, with
    /// Rekordbox's `?` standing in for the volume).
    pub path: String,
    /// The grid, empty when the file carries no `PQTZ`.
    pub truth: GroundTruth,
}

/// Read a big-endian `u32`.
fn be_u32(bytes: &[u8], at: usize) -> Option<u32> {
    let slice = bytes.get(at..at + 4)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Read a big-endian `u16`.
fn be_u16(bytes: &[u8], at: usize) -> Option<u16> {
    let slice = bytes.get(at..at + 2)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

/// Walk the file's sections: `(fourcc, offset, header_len, tag_len)`.
///
/// Bounded by construction — a zero or backwards length ends the
/// walk rather than looping, because this is a binary format from an
/// unknown writer.
fn sections(buf: &[u8]) -> Vec<([u8; 4], usize, usize, usize)> {
    let mut found = Vec::new();
    let Some(mut pos) = be_u32(buf, 4).map(|v| v as usize) else {
        return found;
    };
    while pos + 12 <= buf.len() {
        let Some(fourcc) = buf.get(pos..pos + 4) else {
            break;
        };
        let (Some(header), Some(tag)) = (be_u32(buf, pos + 4), be_u32(buf, pos + 8)) else {
            break;
        };
        let (header, tag) = (header as usize, tag as usize);
        if tag == 0 || header == 0 || header > tag {
            break;
        }
        let mut name = [0u8; 4];
        name.copy_from_slice(fourcc);
        found.push((name, pos, header, tag));
        pos += tag;
    }
    found
}

/// The `PPTH` path, if present.
fn path_of(buf: &[u8], pos: usize) -> Option<String> {
    let len = be_u32(buf, pos + 12)? as usize;
    let raw = buf.get(pos + 16..pos + 16 + len)?;
    let units: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .collect();
    Some(
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_owned(),
    )
}

/// The `PQTZ` grid, if present and self-consistent.
fn grid_of(buf: &[u8], pos: usize, header: usize, tag: usize) -> Option<GroundTruth> {
    let stated = be_u32(buf, pos + 20)? as usize;
    // The file states the count twice over: once as a field, once as
    // the section's length. Disagreement means the offsets are wrong,
    // and a silently wrong grid is worse than none.
    let implied = tag.checked_sub(header)? / BEAT_ENTRY;
    if stated == 0 || stated != implied {
        return None;
    }
    let mut beats = Vec::with_capacity(stated);
    let mut downbeats = Vec::new();
    let mut first_tempo = 0.0;
    for index in 0..stated {
        let at = pos + header + index * BEAT_ENTRY;
        let (Some(in_bar), Some(tempo), Some(ms)) =
            (be_u16(buf, at), be_u16(buf, at + 2), be_u32(buf, at + 4))
        else {
            break;
        };
        let time_s = f64::from(ms) / 1000.0;
        if index == 0 {
            first_tempo = f64::from(tempo) / 100.0;
        }
        if in_bar == 1 {
            downbeats.push(time_s);
        }
        beats.push(time_s);
    }
    if beats.len() != stated {
        return None;
    }
    Some(GroundTruth {
        bpm: first_tempo,
        first_downbeat_ms: downbeats.first().copied().unwrap_or(0.0) * 1000.0,
        beats,
        downbeats,
        boundaries: Vec::new(),
    })
}

/// Parse one `ANLZ0000.DAT`. `None` when it carries no usable grid.
/// Pure — tested against buffers built in the test itself, so no
/// analysed library file has to enter the repository.
#[must_use]
pub fn parse(buf: &[u8]) -> Option<AnlzTrack> {
    if buf.get(0..4)? != b"PMAI" {
        return None;
    }
    let mut path = None;
    let mut truth = None;
    for (fourcc, pos, header, tag) in sections(buf) {
        match &fourcc {
            b"PPTH" => path = path_of(buf, pos),
            b"PQTZ" if truth.is_none() => truth = grid_of(buf, pos, header, tag),
            _ => {}
        }
    }
    Some(AnlzTrack {
        path: path?,
        truth: truth?,
    })
}

/// The file name a `PPTH` path points at — the only part that
/// survives Rekordbox's root marker, and enough to find the audio in
/// a folder the caller knows. Pure — tested.
#[must_use]
pub fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an ANLZ file in memory. Tests own their input; no
    /// analysed track from anyone's library is checked in.
    fn build(path: &str, bpm: f64, beats: usize, first_ms: u32, period_ms: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"PMAI");
        out.extend_from_slice(&28u32.to_be_bytes()); // sections start here
        out.extend_from_slice(&0u32.to_be_bytes()); // file length (unused)
        out.extend_from_slice(&[0u8; 16]); // pad to 28

        // PPTH
        let utf16: Vec<u8> = path
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_be_bytes)
            .collect();
        out.extend_from_slice(b"PPTH");
        out.extend_from_slice(&16u32.to_be_bytes());
        out.extend_from_slice(&((16 + utf16.len()) as u32).to_be_bytes());
        out.extend_from_slice(&(utf16.len() as u32).to_be_bytes());
        out.extend_from_slice(&utf16);

        // PQTZ
        let header = 24usize;
        out.extend_from_slice(b"PQTZ");
        out.extend_from_slice(&(header as u32).to_be_bytes());
        out.extend_from_slice(&((header + beats * BEAT_ENTRY) as u32).to_be_bytes());
        out.extend_from_slice(&[0u8; 8]); // unknown fields
        out.extend_from_slice(&(beats as u32).to_be_bytes());
        for index in 0..beats {
            #[allow(clippy::cast_possible_truncation)]
            let in_bar = (index % 4 + 1) as u16;
            out.extend_from_slice(&in_bar.to_be_bytes());
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            out.extend_from_slice(&((bpm * 100.0) as u16).to_be_bytes());
            out.extend_from_slice(&(first_ms + index as u32 * period_ms).to_be_bytes());
        }
        out
    }

    #[test]
    fn a_grid_is_read_exactly_as_written() {
        let file = build("?/Ross - Buscame.mp3", 124.42, 8, 210, 482);
        let track = parse(&file).expect("a track");
        assert_eq!(track.path, "?/Ross - Buscame.mp3");
        assert_eq!(file_name(&track.path), "Ross - Buscame.mp3");
        assert!((track.truth.bpm - 124.42).abs() < 1e-9);
        assert_eq!(track.truth.beats.len(), 8);
        assert!((track.truth.beats[0] - 0.210).abs() < 1e-9);
        assert!((track.truth.beats[1] - 0.692).abs() < 1e-9);
    }

    #[test]
    fn the_bar_position_gives_the_downbeats() {
        // The reason this format is worth parsing at all: Rekordbox
        // states which beat of the bar every entry is, so bar 1 is
        // read, never inferred.
        let track = parse(&build("?/x.mp3", 120.0, 8, 0, 500)).expect("a track");
        assert_eq!(track.truth.downbeats.len(), 2, "every fourth beat");
        assert!((track.truth.downbeats[0] - 0.0).abs() < 1e-9);
        assert!((track.truth.downbeats[1] - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_count_that_disagrees_with_the_length_is_refused() {
        // THE guard. The first version of this parser read the count
        // from the wrong offset and reported 524288 beats at 8.49 BPM
        // for a file holding 849 at 124.42. A grid that wrong must
        // never reach a metric — the two statements of the count have
        // to agree or the section is dropped.
        let mut file = build("?/x.mp3", 124.42, 8, 210, 482);
        // Corrupt the stated count; the section length still says 8.
        let at = file.len() - 8 * BEAT_ENTRY - 4;
        file[at..at + 4].copy_from_slice(&999u32.to_be_bytes());
        assert!(
            parse(&file).is_none(),
            "a disagreeing count must be refused"
        );
    }

    #[test]
    fn rubbish_is_refused_rather_than_guessed_at() {
        assert!(parse(b"").is_none());
        assert!(parse(b"NOPE").is_none());
        // Right magic, no sections at all.
        let mut bare = b"PMAI".to_vec();
        bare.extend_from_slice(&28u32.to_be_bytes());
        bare.extend_from_slice(&[0u8; 24]);
        assert!(parse(&bare).is_none(), "no path and no grid is not a track");
    }

    #[test]
    fn a_zero_length_section_cannot_loop_forever() {
        // A binary format from an unknown writer: a bad length must
        // end the walk, not spin.
        let mut file = b"PMAI".to_vec();
        file.extend_from_slice(&12u32.to_be_bytes());
        file.extend_from_slice(&0u32.to_be_bytes());
        file.extend_from_slice(b"PQTZ");
        file.extend_from_slice(&0u32.to_be_bytes()); // header 0
        file.extend_from_slice(&0u32.to_be_bytes()); // tag 0
        assert!(parse(&file).is_none());
    }
}
