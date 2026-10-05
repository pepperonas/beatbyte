//! `beatbyte-cli shots compare <reference> <new>`: are two harness
//! screenshots the same picture?
//!
//! The screenshot harness (`BEATBYTE_SHOT_STATE`, `BEATBYTE_SHOT_DIR`)
//! photographs a screen; this compares two such photographs, or two
//! folders of them by file name, and fails when more than a small
//! share of the pixels changed. It is how a refactor of a screen
//! proves it left the screen alone (`tools/shot-check.sh` runs both
//! halves).
//!
//! A pixel counts as changed when any channel moves by more than
//! [`CHANNEL_TOLERANCE`] — a rounding difference in a blend is not a
//! change, a moved edge or a different colour is. Two runs of the same
//! build give identical files once the harness waits for the tube to
//! open, the UI scale lands exactly on its target and the photographed
//! row is held (all fixed in 0.18.69) — so by default not one pixel may
//! change ([`DEFAULT_MAX_SHARE`]). A screen that animates on its own
//! needs `--max-share`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// How far a channel may move before the pixel counts as changed.
pub const CHANNEL_TOLERANCE: u8 = 24;

/// The share of changed pixels allowed by default: none. A tolerance
/// was tried and measured against: one letter removed from a subtitle
/// changes 0.062 % of a 2560x1600 picture, so even 0.05 % would let a
/// real edit through as "same".
pub const DEFAULT_MAX_SHARE: f64 = 0.0;

/// What a comparison found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Difference {
    /// Pixels that changed.
    pub changed: u64,
    /// Pixels compared.
    pub total: u64,
    /// The smallest box holding every changed pixel, `(x0, y0, x1, y1)`
    /// inclusive.
    pub bounds: Option<(u32, u32, u32, u32)>,
}

impl Difference {
    /// The changed share, 0..=1.
    #[must_use]
    pub fn share(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.changed as f64 / self.total as f64
        }
    }
}

/// Compare two RGBA pictures of `width` × `height`. Pure — tested.
///
/// # Errors
/// When either buffer is not `width * height * 4` bytes long.
pub fn compare(a: &[u8], b: &[u8], width: u32, height: u32) -> Result<Difference, String> {
    let wanted = width as usize * height as usize * 4;
    if a.len() != wanted || b.len() != wanted {
        return Err(format!(
            "a {width}x{height} picture is {wanted} bytes; got {} and {}",
            a.len(),
            b.len()
        ));
    }
    let mut changed = 0u64;
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (index, (p, q)) in a
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .enumerate()
    {
        let moved = p
            .iter()
            .zip(q)
            .any(|(x, y)| x.abs_diff(*y) > CHANNEL_TOLERANCE);
        if !moved {
            continue;
        }
        changed += 1;
        let (x, y) = (
            (index % width as usize) as u32,
            (index / width as usize) as u32,
        );
        bounds = Some(match bounds {
            None => (x, y, x, y),
            Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
        });
    }
    Ok(Difference {
        changed,
        total: u64::from(width) * u64::from(height),
        bounds,
    })
}

/// Read a PNG as RGBA.
fn load(path: &Path) -> Result<image::RgbaImage, String> {
    image::open(path)
        .map(|picture| picture.to_rgba8())
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Compare two files. A size mismatch is a difference of everything:
/// a picture at another resolution is not the same picture.
fn compare_files(reference: &Path, new: &Path) -> Result<Difference, String> {
    let (a, b) = (load(reference)?, load(new)?);
    if a.dimensions() != b.dimensions() {
        return Err(format!(
            "sizes differ: {:?} against {:?} (the window or its display changed)",
            a.dimensions(),
            b.dimensions()
        ));
    }
    compare(a.as_raw(), b.as_raw(), a.width(), a.height())
}

/// The pairs to compare: two files, or every PNG of the reference
/// folder against the file of that name in the new one. Pure over the
/// listed names — tested.
#[must_use]
pub fn pairs(reference: &Path, new: &Path, names: &[String]) -> Vec<(PathBuf, PathBuf)> {
    if names.is_empty() {
        return vec![(reference.to_path_buf(), new.to_path_buf())];
    }
    names
        .iter()
        .map(|name| (reference.join(name), new.join(name)))
        .collect()
}

/// `shots compare`.
pub fn run_compare(reference: &Path, new: &Path, max_share: f64) -> ExitCode {
    let names: Vec<String> = if reference.is_dir() {
        let mut names: Vec<String> = std::fs::read_dir(reference)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.to_ascii_lowercase().ends_with(".png"))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        if names.is_empty() {
            eprintln!("{}: no PNG to compare against", reference.display());
            return ExitCode::FAILURE;
        }
        names
    } else {
        Vec::new()
    };
    let mut failed = 0usize;
    for (a, b) in pairs(reference, new, &names) {
        let name = a
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        match compare_files(&a, &b) {
            Ok(difference) => {
                let share = difference.share();
                let verdict = if share <= max_share {
                    "same"
                } else {
                    "CHANGED"
                };
                if share > max_share {
                    failed += 1;
                }
                let bounds = difference
                    .bounds
                    .map(|(x0, y0, x1, y1)| format!(", in x {x0}..{x1} y {y0}..{y1}"))
                    .unwrap_or_default();
                println!(
                    "{verdict:8} {name}: {} of {} pixels ({:.4} %){bounds}",
                    difference.changed,
                    difference.total,
                    share * 100.0
                );
            }
            Err(reason) => {
                failed += 1;
                println!("CHANGED  {name}: {reason}");
            }
        }
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "{failed} picture(s) changed by more than {:.4} % — look at them before deciding",
            max_share * 100.0
        );
        ExitCode::FAILURE
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn picture(width: u32, height: u32, fill: [u8; 4]) -> Vec<u8> {
        fill.repeat((width * height) as usize)
    }

    #[test]
    fn the_same_picture_is_the_same() {
        let a = picture(8, 4, [10, 20, 30, 255]);
        let difference = compare(&a, &a, 8, 4).unwrap();
        assert_eq!(difference.changed, 0);
        assert_eq!(difference.bounds, None);
        assert!(difference.share().abs() < f64::EPSILON);
    }

    #[test]
    fn a_rounding_wobble_is_not_a_change_and_a_new_colour_is() {
        let a = picture(4, 4, [100, 100, 100, 255]);
        let mut b = a.clone();
        // One channel nudged by the tolerance: still the same pixel.
        b[0] = 100 + CHANNEL_TOLERANCE;
        assert_eq!(compare(&a, &b, 4, 4).unwrap().changed, 0);
        // One more and it is a different colour.
        b[0] = 101 + CHANNEL_TOLERANCE;
        assert_eq!(compare(&a, &b, 4, 4).unwrap().changed, 1);
    }

    #[test]
    fn the_box_holds_every_change() {
        let a = picture(10, 6, [0, 0, 0, 255]);
        let mut b = a.clone();
        let mut set = |x: usize, y: usize| b[(y * 10 + x) * 4 + 1] = 200;
        set(2, 1);
        set(7, 4);
        let difference = compare(&a, &b, 10, 6).unwrap();
        assert_eq!(difference.changed, 2);
        assert_eq!(difference.bounds, Some((2, 1, 7, 4)));
        assert!((difference.share() - 2.0 / 60.0).abs() < 1e-12);
    }

    #[test]
    fn a_buffer_of_the_wrong_size_is_refused() {
        let a = picture(4, 4, [0, 0, 0, 255]);
        assert!(compare(&a, &a[..8], 4, 4).is_err());
    }

    #[test]
    fn folders_pair_by_name_and_files_pair_as_given() {
        let names = vec!["a.png".to_owned(), "b.png".to_owned()];
        let pairs_of = pairs(Path::new("/ref"), Path::new("/new"), &names);
        assert_eq!(
            pairs_of[1],
            (PathBuf::from("/ref/b.png"), PathBuf::from("/new/b.png"))
        );
        let single = pairs(Path::new("/ref.png"), Path::new("/new.png"), &[]);
        assert_eq!(
            single,
            vec![(PathBuf::from("/ref.png"), PathBuf::from("/new.png"))]
        );
    }
}
