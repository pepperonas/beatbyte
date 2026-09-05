//! The models this build knows about — and will accept.
//!
//! Compiled in, not fetched: a build knows exactly which bytes it will
//! run. Each entry pins the URL (a release asset of the project's own
//! repository), the exact size and the SHA-256; the store refuses
//! anything else. The licence is recorded here and in
//! `docs/development/asset-licenses.md` the moment a model is added,
//! not when it is first used.
//!
//! The registry is empty in milestone L1: the crate is the runtime,
//! and the aligner (L2) registers the first real model together with
//! its licence. Tests build their own [`ModelSpec`]s.

/// One model the build knows how to fetch, verify and load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Stable id, also the folder name under the store: lowercase,
    /// `[a-z0-9-]`, e.g. `wav2vec2-base-960h`.
    pub id: &'static str,
    /// The file name inside that folder.
    pub file: &'static str,
    /// Where the bytes come from. HTTPS, a URL the project controls.
    pub url: &'static str,
    /// The exact size in bytes; a download is capped at it.
    pub bytes: u64,
    /// SHA-256 of the file, lowercase hex, 64 characters.
    pub sha256: &'static str,
    /// The model's licence (SPDX id or a short name).
    pub licence: &'static str,
    /// One line on what the model is for.
    pub purpose: &'static str,
}

/// Every model this build can install.
pub const REGISTRY: &[ModelSpec] = &[];

/// Look a model up by id.
#[must_use]
pub fn spec(id: &str) -> Option<&'static ModelSpec> {
    REGISTRY.iter().find(|spec| spec.id == id)
}

/// Whether a spec is well-formed: the checks the registry's own test
/// runs over every entry, exposed so a caller can run them over a
/// spec it built itself. Returns the first problem found.
#[must_use]
pub fn problem(spec: &ModelSpec) -> Option<String> {
    if spec.id.is_empty()
        || !spec
            .id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Some(format!("id `{}` is not lowercase [a-z0-9-]", spec.id));
    }
    if spec.file.is_empty()
        || spec.file.contains('/')
        || spec.file.contains('\\')
        || spec.file.contains("..")
    {
        return Some(format!("file `{}` must be a plain file name", spec.file));
    }
    if !spec.url.starts_with("https://") {
        return Some(format!("url `{}` is not https", spec.url));
    }
    if spec.bytes == 0 {
        return Some("size is zero".to_owned());
    }
    if spec.sha256.len() != 64
        || !spec
            .sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Some("sha256 is not 64 lowercase hex digits".to_owned());
    }
    if spec.licence.trim().is_empty() {
        return Some("licence is empty".to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_model_is_well_formed_and_unique() {
        for spec in REGISTRY {
            assert_eq!(problem(spec), None, "{}", spec.id);
        }
        let mut ids: Vec<&str> = REGISTRY.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), REGISTRY.len(), "duplicate ids");
        assert_eq!(spec("no-such-model"), None);
    }

    #[test]
    fn the_checks_bite() {
        let good = ModelSpec {
            id: "dummy-1",
            file: "dummy.onnx",
            url: "https://github.com/pepperonas/beatbyte/releases/download/models-v1/dummy.onnx",
            bytes: 10,
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            licence: "MIT",
            purpose: "test",
        };
        assert_eq!(problem(&good), None);
        assert!(
            problem(&ModelSpec {
                id: "Dummy",
                ..good
            })
            .is_some(),
            "uppercase id"
        );
        assert!(
            problem(&ModelSpec {
                file: "../x",
                ..good
            })
            .is_some(),
            "path in file"
        );
        assert!(
            problem(&ModelSpec {
                url: "http://x",
                ..good
            })
            .is_some(),
            "plain http"
        );
        assert!(problem(&ModelSpec { bytes: 0, ..good }).is_some(), "empty");
        assert!(
            problem(&ModelSpec {
                sha256: "abc",
                ..good
            })
            .is_some(),
            "short hash"
        );
        assert!(
            problem(&ModelSpec {
                sha256: "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855",
                ..good
            })
            .is_some(),
            "uppercase hash"
        );
        assert!(
            problem(&ModelSpec {
                licence: " ",
                ..good
            })
            .is_some(),
            "no licence"
        );
    }
}
