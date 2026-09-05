//! The encoder delay a lossy container declares — and that the
//! decoder does not honour.
//!
//! AAC encoders emit *priming* frames before the first real sample
//! (Apple: 2112, FFmpeg: 1024) and declare them in the MP4 container so
//! a player can skip them: Apple in the `iTunSMPB` tag, everyone in an
//! `edts`/`elst` edit list. Symphonia 0.5.5 parses the edit list and
//! never applies it (measured: `docs/audio/decode-offset.md`), so the
//! priming came out as 23–48 ms of leading audio in every `.m4a` the
//! game decoded — in analysis *and* playback alike, which kept charts
//! and audio in step with each other and everything timed against the
//! master (an `.lrc`, a word alignment) out of step with both.
//!
//! This module reads the declaration so the two decode paths can skip
//! it, sample-exactly. It is a box walker over untrusted input: every
//! size is bounds-checked against the file, depth and box counts are
//! capped, and nothing is allocated from a declared size. Anything it
//! does not understand is "no priming", never an error — a song that
//! cannot be read for its priming still plays, as it did before.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// How many bytes of a `moov` box this reader will load. Real `moov`
/// boxes are tens of kilobytes; a declared size beyond this means a
/// file this reader will not trust.
const MAX_MOOV_BYTES: u64 = 16 * 1024 * 1024;
/// Boxes examined per container level before giving up.
const MAX_BOXES_PER_LEVEL: usize = 256;
/// Nesting depth searched below `moov`.
const MAX_DEPTH: usize = 8;
/// A priming longer than this is not priming.
const MAX_PRIMING_SAMPLES: u64 = 1 << 20;

/// What the container declares about its audio track's timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Priming {
    /// Frames (per channel) the decoder delivers BEFORE the master's
    /// first sample, in the audio track's own timescale.
    pub samples: u32,
    /// The timescale those frames are counted in — for MP4 audio the
    /// sample rate.
    pub timescale: u32,
}

impl Priming {
    /// The priming as a duration in seconds; 0 when nothing is
    /// declared.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.timescale == 0 {
            0.0
        } else {
            f64::from(self.samples) / f64::from(self.timescale)
        }
    }
}

/// The priming a file declares, or [`Priming::default`] when it
/// declares none, is not an MP4, or cannot be read. Never an error.
#[must_use]
pub fn container_priming(path: &Path) -> Priming {
    let Ok(mut file) = File::open(path) else {
        return Priming::default();
    };
    let Ok(len) = file.seek(SeekFrom::End(0)) else {
        return Priming::default();
    };
    // Top level: walk boxes until `moov`, which may sit after a
    // multi-hundred-megabyte `mdat` — seek past, never read.
    let mut offset = 0u64;
    for _ in 0..MAX_BOXES_PER_LEVEL {
        if offset + 8 > len {
            return Priming::default();
        }
        let Ok(header) = read_header(&mut file, offset, len) else {
            return Priming::default();
        };
        if &header.kind == b"moov" {
            let body_len = header.size - u64::from(header.header_len);
            if body_len > MAX_MOOV_BYTES {
                return Priming::default();
            }
            let mut body = vec![0u8; body_len as usize];
            if file
                .seek(SeekFrom::Start(offset + u64::from(header.header_len)))
                .is_err()
                || file.read_exact(&mut body).is_err()
            {
                return Priming::default();
            }
            return priming_in_moov(&body);
        }
        // The first box must be `ftyp` for this to be an MP4 at all;
        // a WAV, MP3 or FLAC never gets past here.
        if offset == 0 && &header.kind != b"ftyp" {
            return Priming::default();
        }
        offset += header.size;
    }
    Priming::default()
}

struct Header {
    kind: [u8; 4],
    /// Whole box size including the header.
    size: u64,
    header_len: u8,
}

fn read_header(file: &mut File, offset: u64, len: u64) -> std::io::Result<Header> {
    file.seek(SeekFrom::Start(offset))?;
    let mut head = [0u8; 8];
    file.read_exact(&mut head)?;
    let mut size = u64::from(u32::from_be_bytes([head[0], head[1], head[2], head[3]]));
    let kind = [head[4], head[5], head[6], head[7]];
    let mut header_len = 8u8;
    if size == 1 {
        let mut large = [0u8; 8];
        file.read_exact(&mut large)?;
        size = u64::from_be_bytes(large);
        header_len = 16;
    } else if size == 0 {
        size = len - offset;
    }
    if size < u64::from(header_len) || offset + size > len {
        return Err(std::io::Error::other("box runs past the file"));
    }
    Ok(Header {
        kind,
        size,
        header_len,
    })
}

/// A box inside an in-memory buffer: its type and its body's range.
#[derive(Clone, Copy)]
struct Boxed<'a> {
    kind: [u8; 4],
    body: &'a [u8],
}

/// The boxes laid end to end in `data`, bounds-checked. A malformed
/// size ends the iteration rather than the program.
fn children(data: &[u8]) -> impl Iterator<Item = Boxed<'_>> {
    let mut at = 0usize;
    let mut count = 0usize;
    core::iter::from_fn(move || {
        if count >= MAX_BOXES_PER_LEVEL || at + 8 > data.len() {
            return None;
        }
        count += 1;
        let size = u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
        let kind = [data[at + 4], data[at + 5], data[at + 6], data[at + 7]];
        let (size, header_len) = match size {
            1 => {
                if at + 16 > data.len() {
                    return None;
                }
                let mut large = [0u8; 8];
                large.copy_from_slice(&data[at + 8..at + 16]);
                (usize::try_from(u64::from_be_bytes(large)).ok()?, 16usize)
            }
            0 => (data.len() - at, 8),
            n => (n as usize, 8),
        };
        if size < header_len || at + size > data.len() {
            return None;
        }
        let body = &data[at + header_len..at + size];
        at += size;
        Some(Boxed { kind, body })
    })
}

/// Look for the priming under `moov`: `iTunSMPB` first (exact, what
/// Apple's own player uses), then the audio track's edit list.
fn priming_in_moov(moov: &[u8]) -> Priming {
    if let Some(samples) = itun_smpb(moov, 0) {
        // iTunSMPB counts codec samples, which is the track's own
        // timescale for MP4 audio; the decoder reports the same rate.
        return Priming {
            samples,
            timescale: track_timescale(moov).unwrap_or(0),
        };
    }
    for trak in children(moov).filter(|b| &b.kind == b"trak") {
        if let Some(priming) = track_edit_list(trak.body) {
            return priming;
        }
    }
    Priming::default()
}

/// `moov/udta/meta/ilst/----` with `name` = `iTunSMPB`: a hex text of
/// which the second field is the priming and the fourth the valid
/// length. Returns the priming in samples.
fn itun_smpb(data: &[u8], depth: usize) -> Option<u32> {
    if depth > MAX_DEPTH {
        return None;
    }
    for b in children(data) {
        match &b.kind {
            b"udta" | b"ilst" => {
                if let Some(found) = itun_smpb(b.body, depth + 1) {
                    return Some(found);
                }
            }
            // `meta` is a FULL box: four bytes of version/flags
            // precede its children.
            b"meta" => {
                if b.body.len() >= 4
                    && let Some(found) = itun_smpb(&b.body[4..], depth + 1)
                {
                    return Some(found);
                }
            }
            b"----" => {
                let mut is_smpb = false;
                let mut payload: Option<&[u8]> = None;
                for inner in children(b.body) {
                    match &inner.kind {
                        // `name`: version/flags, then the ASCII name.
                        b"name" => {
                            is_smpb = inner.body.len() >= 4 && &inner.body[4..] == b"iTunSMPB";
                        }
                        // `data`: type + locale (8 bytes), then text.
                        b"data" if inner.body.len() >= 8 => {
                            payload = Some(&inner.body[8..]);
                        }
                        _ => {}
                    }
                }
                if is_smpb
                    && let Some(text) = payload
                    && let Some(samples) = smpb_priming(text)
                {
                    return Some(samples);
                }
            }
            _ => {}
        }
    }
    None
}

/// The priming field of an `iTunSMPB` text: " 00000000 00000840 000000D4 …",
/// hexadecimal, second field.
fn smpb_priming(text: &[u8]) -> Option<u32> {
    let text = core::str::from_utf8(text).ok()?;
    let field = text.split_ascii_whitespace().nth(1)?;
    let samples = u64::from_str_radix(field, 16).ok()?;
    (samples <= MAX_PRIMING_SAMPLES).then_some(samples as u32)
}

/// The first audio track's `mdhd` timescale under a `moov` body.
fn track_timescale(moov: &[u8]) -> Option<u32> {
    children(moov)
        .filter(|b| &b.kind == b"trak")
        .find_map(|trak| mdhd_timescale(trak.body))
}

/// `trak/mdia/mdhd` timescale.
fn mdhd_timescale(trak: &[u8]) -> Option<u32> {
    let mdia = children(trak).find(|b| &b.kind == b"mdia")?;
    let mdhd = children(mdia.body).find(|b| &b.kind == b"mdhd")?;
    let body = mdhd.body;
    // version(1) flags(3) then, for v0: created(4) modified(4)
    // timescale(4); for v1: created(8) modified(8) timescale(4).
    let at = match body.first()? {
        0 => 12,
        1 => 20,
        _ => return None,
    };
    let bytes = body.get(at..at + 4)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// The track's edit list, when it has one whose first entry starts
/// inside the media (a positive `media_time`): that is the encoder
/// delay, in the media's timescale.
fn track_edit_list(trak: &[u8]) -> Option<Priming> {
    let timescale = mdhd_timescale(trak)?;
    let edts = children(trak).find(|b| &b.kind == b"edts")?;
    let elst = children(edts.body).find(|b| &b.kind == b"elst")?;
    let body = elst.body;
    let version = *body.first()?;
    let count = u32::from_be_bytes([*body.get(4)?, *body.get(5)?, *body.get(6)?, *body.get(7)?]);
    let mut at = 8usize;
    for _ in 0..count.min(16) {
        let media_time: i64 = match version {
            0 => {
                let bytes = body.get(at + 4..at + 8)?;
                at += 12;
                i64::from(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
            1 => {
                let bytes = body.get(at + 8..at + 16)?;
                at += 20;
                i64::from_be_bytes([
                    bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
                ])
            }
            _ => return None,
        };
        // -1 is an empty edit (leading silence), not priming.
        if media_time >= 0 {
            let samples = u64::try_from(media_time).ok()?;
            if samples > MAX_PRIMING_SAMPLES {
                return None;
            }
            return Some(Priming {
                samples: samples as u32,
                timescale,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    #[test]
    fn apple_declares_its_priming_in_itunsmpb() {
        let priming = container_priming(&fixture("click-apple.m4a"));
        assert_eq!(priming.samples, 2112);
        assert_eq!(priming.timescale, 44_100);
        assert!((priming.seconds() - 0.047_891).abs() < 1e-6);
    }

    #[test]
    fn ffmpeg_declares_its_priming_in_the_edit_list() {
        let priming = container_priming(&fixture("click-ffmpeg.m4a"));
        assert_eq!(priming.samples, 1024);
        assert_eq!(priming.timescale, 44_100);
    }

    #[test]
    fn other_containers_declare_nothing() {
        for name in ["click-lame.mp3", "tone.wav", "tone.flac", "tone.ogg"] {
            assert_eq!(
                container_priming(&fixture(name)),
                Priming::default(),
                "{name}"
            );
        }
        assert_eq!(
            container_priming(Path::new("/definitely/not/here.m4a")),
            Priming::default()
        );
    }

    #[test]
    fn the_older_tone_fixture_is_an_apple_encode_too() {
        // `tone.m4a` was made with afconvert for the format tests; it
        // carries the same 2112 — a second, independent Apple sample.
        assert_eq!(container_priming(&fixture("tone.m4a")).samples, 2112);
    }

    #[test]
    fn garbage_is_no_priming_and_no_panic() {
        let dir = std::env::temp_dir().join(format!("beatbyte-priming-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let cases: [(&str, Vec<u8>); 4] = [
            ("empty.m4a", Vec::new()),
            ("short.m4a", b"ftyp".to_vec()),
            // A moov claiming to be far larger than the file.
            ("liar.m4a", {
                let mut v = Vec::new();
                v.extend_from_slice(&20u32.to_be_bytes());
                v.extend_from_slice(b"ftypM4A ");
                v.extend_from_slice(&[0u8; 8]);
                v.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
                v.extend_from_slice(b"moov");
                v
            }),
            // A moov of zero-size boxes: must terminate.
            ("loop.m4a", {
                let mut v = Vec::new();
                v.extend_from_slice(&16u32.to_be_bytes());
                v.extend_from_slice(b"ftypM4A ");
                v.extend_from_slice(&[0u8; 4]);
                v.extend_from_slice(&24u32.to_be_bytes());
                v.extend_from_slice(b"moov");
                v.extend_from_slice(&7u32.to_be_bytes());
                v.extend_from_slice(b"trak");
                v.extend_from_slice(&[0u8; 8]);
                v
            }),
        ];
        for (name, bytes) in cases {
            let path = dir.join(name);
            std::fs::write(&path, bytes).expect("write");
            assert_eq!(container_priming(&path), Priming::default(), "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_smpb_text_is_read_by_field_not_by_position() {
        assert_eq!(
            smpb_priming(b" 00000000 00000840 000000D4 00000000000766EC"),
            Some(2112)
        );
        assert_eq!(smpb_priming(b"00000000 00000400"), Some(1024));
        assert_eq!(smpb_priming(b"00000000"), None, "no second field");
        assert_eq!(smpb_priming(b"zz zz"), None, "not hex");
        assert_eq!(smpb_priming(b"0 FFFFFFFF"), None, "absurd");
    }
}
