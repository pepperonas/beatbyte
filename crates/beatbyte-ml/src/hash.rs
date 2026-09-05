//! SHA-256, the identity of a model file and of anything derived
//! from one.

use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

/// SHA-256 of a byte slice, lowercase hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// SHA-256 of a file, streamed — a model is hundreds of megabytes and
/// never needs to be in memory whole to be identified.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// A running SHA-256 for data that arrives in pieces (a download).
pub struct Hashing(Sha256);

impl Hashing {
    /// Start a fresh digest.
    #[must_use]
    pub fn new() -> Hashing {
        Hashing(Sha256::new())
    }

    /// Feed the next piece.
    pub fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    /// The digest of everything fed so far, lowercase hex.
    #[must_use]
    pub fn finish(self) -> String {
        hex(&self.0.finalize())
    }
}

impl Default for Hashing {
    fn default() -> Hashing {
        Hashing::new()
    }
}

fn hex(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // FIPS 180-4 / NIST vectors: the implementation is a dependency,
    // but the wrapper's hex, streaming and piecewise paths are ours.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn known_vectors() {
        assert_eq!(sha256_hex(b"abc"), ABC);
        assert_eq!(sha256_hex(b""), EMPTY);
        assert_eq!(
            sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn a_file_and_the_pieces_of_it_hash_the_same_as_the_whole() {
        let dir = std::env::temp_dir().join(format!("beatbyte-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        // Larger than the streaming buffer, so more than one read.
        let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let path = dir.join("blob");
        std::fs::write(&path, &bytes).expect("write");
        let whole = sha256_hex(&bytes);
        assert_eq!(sha256_file(&path).expect("hash file"), whole);
        let mut piecewise = Hashing::new();
        for chunk in bytes.chunks(7_777) {
            piecewise.update(chunk);
        }
        assert_eq!(piecewise.finish(), whole);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
