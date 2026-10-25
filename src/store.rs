//! On-disk state, one API two ways: desktop builds use the platform's data directory
//! (via `directories::ProjectDirs` for "com.fm1-grid"), wasm32 builds use the browser's
//! localStorage. Objects are JSON (serde); blobs are plain bytes (base64 in the
//! browser). Paths are slash-separated and relative — `..`, absolute paths and
//! backslashes are rejected at the boundary.
//!
//! Desktop layout: `<data dir>/objects/<path>.json` and `<data dir>/blobs/<path>`.

use serde::Serialize;
use serde::de::DeserializeOwned;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};

/// The store root: the app's data directory. Construct once, share by reference.
pub(crate) struct Store {
    #[cfg(not(target_arch = "wasm32"))]
    root: PathBuf,
}

/// `*` matching, the whole string (no `?`, no character classes — the store's paths
/// are app-chosen, so this stays tiny).
#[cfg_attr(not(test), allow(dead_code))]
fn wildcard_match(pattern: &str, text: &str) -> bool {
    wildcard_match_bytes(pattern.as_bytes(), text.as_bytes())
}

fn wildcard_match_bytes(pattern: &[u8], text: &[u8]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some((b'*', rest)) => {
            (0..=text.len()).any(|skip| wildcard_match_bytes(rest, &text[skip..]))
        }
        Some((&c, rest)) => {
            !text.is_empty() && text[0] == c && wildcard_match_bytes(rest, &text[1..])
        }
    }
}

/// A store path must be relative and plain: no `..`, no leading slash, no backslash.
fn check_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.split('/').any(|part| part == "..")
    {
        return Err(format!("bad store path {path:?}"));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
impl Store {
    /// The app's data store under the platform's data directory
    /// (~/.local/share/fm1-grid on Linux). FM1_GRID_DATA_DIR overrides it (tests).
    pub fn new() -> Result<Self, String> {
        if let Some(dir) = std::env::var_os("FM1_GRID_DATA_DIR") {
            return Ok(Self::in_dir(Path::new(&dir)));
        }
        let dirs = directories::ProjectDirs::from("com", "fm1-grid", "fm1-grid")
            .ok_or("no home directory for the data store")?;
        Ok(Self::in_dirs(dirs.data_dir().to_path_buf()))
    }

    /// A store rooted at an arbitrary directory (tests).
    pub fn in_dir(dir: &Path) -> Self {
        Self::in_dirs(dir.to_path_buf())
    }

    fn in_dirs(root: PathBuf) -> Self {
        std::fs::create_dir_all(root.join("objects")).ok();
        std::fs::create_dir_all(root.join("blobs")).ok();
        Self { root }
    }

    fn object_file(&self, path: &str) -> Result<PathBuf, String> {
        check_path(path)?;
        Ok(self.root.join("objects").join(format!("{path}.json")))
    }

    fn blob_file(&self, path: &str) -> Result<PathBuf, String> {
        check_path(path)?;
        Ok(self.root.join("blobs").join(path))
    }

    /// The names (without `.json`) of every stored object, optionally filtered.
    /// (The listing side of the API serves the library tab; pins only fetch/store.)
    #[allow(dead_code)]
    pub fn list_objects(&self, wildcard: Option<&str>) -> Vec<String> {
        let mut out = Vec::new();
        walk(&self.root.join("objects"), "", &mut |rel| {
            if let Some(name) = rel.strip_suffix(".json") {
                if wildcard.is_none_or(|w| wildcard_match(w, name)) {
                    out.push(name.to_string());
                }
            }
        });
        out.sort();
        out
    }

    /// The objects that parse as `T`.
    #[allow(dead_code)]
    pub fn list_objects_of_type<T: DeserializeOwned>(&self, wildcard: Option<&str>) -> Vec<String> {
        self.list_objects(wildcard)
            .into_iter()
            .filter(|name| self.fetch_object::<T>(name).is_ok())
            .collect()
    }

    pub fn store_object<T: Serialize>(&self, path: &str, object: &T) -> Result<(), String> {
        let file = self.object_file(path)?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_vec_pretty(object).map_err(|e| e.to_string())?;
        std::fs::write(&file, json).map_err(|e| format!("store {path}: {e}"))
    }

    pub fn fetch_object<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let file = self.object_file(path)?;
        let bytes =
            std::fs::read(&file).map_err(|e| format!("fetch {path}: {e}"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("parse {path}: {e}"))
    }

    /// Every stored blob's path, optionally filtered.
    #[allow(dead_code)]
    pub fn list_blobs(&self, wildcard: Option<&str>) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        walk(&self.root.join("blobs"), "", &mut |rel| {
            if wildcard.is_none_or(|w| wildcard_match(w, rel)) {
                out.push(rel.to_string());
            }
        });
        out.sort();
        Ok(out)
    }

    pub fn store_blob(&self, path: &str, blob: &[u8]) -> Result<(), String> {
        let file = self.blob_file(path)?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&file, blob).map_err(|e| format!("store blob {path}: {e}"))
    }

    pub fn fetch_blob(&self, path: &str) -> Result<Vec<u8>, String> {
        let file = self.blob_file(path)?;
        std::fs::read(&file).map_err(|e| format!("fetch blob {path}: {e}"))
    }
}

/// Recurse a directory, reporting each file's path relative to `root`.
#[cfg(not(target_arch = "wasm32"))]
fn walk(dir: &Path, prefix: &str, visit: &mut impl FnMut(&str)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            walk(&path, &rel, visit);
        } else {
            visit(&rel);
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_store {
    use super::*;
    use web_sys::window;

    const PREFIX: &str = "fm1-grid:";

    fn storage() -> Result<web_sys::Storage, String> {
        window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or_else(|| "no localStorage".to_string())
    }

    fn b64_encode(bytes: &[u8]) -> Result<String, String> {
        // btoa takes a byte string: one char per byte.
        let text: String = bytes.iter().map(|&b| char::from(b)).collect();
        window()
            .and_then(|w| w.btoa(&text).ok())
            .ok_or_else(|| "btoa failed".to_string())
    }

    fn b64_decode(text: &str) -> Result<Vec<u8>, String> {
        let s = window()
            .and_then(|w| w.atob(text).ok())
            .ok_or_else(|| "atob failed".to_string())?;
        Ok(s.chars().map(|c| c as u8).collect())
    }

    impl Store {
        pub fn new() -> Result<Self, String> {
            storage()?;
            Ok(Self {})
        }

        fn keys(kind: &str, wildcard: Option<&str>) -> Vec<String> {
            let Ok(storage) = storage() else { return vec![] };
            let prefix = format!("{PREFIX}{kind}:");
            let mut out = Vec::new();
            for i in 0..storage.length().unwrap_or(0) {
                if let Ok(Some(key)) = storage.key(i) {
                    if let Some(name) = key.strip_prefix(&prefix) {
                        if wildcard.is_none_or(|w| wildcard_match(w, name)) {
                            out.push(name.to_string());
                        }
                    }
                }
            }
            out.sort();
            out
        }

        pub fn list_objects(&self, wildcard: Option<&str>) -> Vec<String> {
            Self::keys("o", wildcard)
        }

        pub fn list_objects_of_type<T: DeserializeOwned>(&self, wildcard: Option<&str>) -> Vec<String> {
            self.list_objects(wildcard)
                .into_iter()
                .filter(|name| self.fetch_object::<T>(name).is_ok())
                .collect()
        }

        pub fn store_object<T: Serialize>(&self, path: &str, object: &T) -> Result<(), String> {
            check_path(path)?;
            let json = serde_json::to_string(object).map_err(|e| e.to_string())?;
            storage()?
                .set_item(&format!("{PREFIX}o:{path}"), &json)
                .map_err(|_| format!("store {path}: localStorage refused"))
        }

        pub fn fetch_object<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
            check_path(path)?;
            let json = storage()?
                .get_item(&format!("{PREFIX}o:{path}"))
                .map_err(|_| "localStorage read".to_string())?
                .ok_or_else(|| format!("fetch {path}: not stored"))?;
            serde_json::from_str(&json).map_err(|e| format!("parse {path}: {e}"))
        }

        pub fn list_blobs(&self, wildcard: Option<&str>) -> Result<Vec<String>, String> {
            Ok(Self::keys("b", wildcard))
        }

        pub fn store_blob(&self, path: &str, blob: &[u8]) -> Result<(), String> {
            check_path(path)?;
            storage()?
                .set_item(&format!("{PREFIX}b:{path}"), &b64_encode(blob)?)
                .map_err(|_| format!("store blob {path}: localStorage refused"))
        }

        pub fn fetch_blob(&self, path: &str) -> Result<Vec<u8>, String> {
            check_path(path)?;
            let text = storage()?
                .get_item(&format!("{PREFIX}b:{path}"))
                .map_err(|_| "localStorage read".to_string())?
                .ok_or_else(|| format!("fetch blob {path}: not stored"))?;
            b64_decode(&text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn temp_store(tag: &str) -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "fm1-grid-store-test-{}-{}",
            std::process::id(),
            tag
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (Store::in_dir(&dir), dir)
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Named(String);

    #[test]
    fn object_round_trip() {
        let (store, dir) = temp_store("objects");
        store.store_object("a/point", &Point { x: 1, y: 2 }).unwrap();
        assert_eq!(
            store.fetch_object::<Point>("a/point").unwrap(),
            Point { x: 1, y: 2 }
        );
        assert_eq!(store.list_objects(None), vec!["a/point"]);
        assert_eq!(store.list_objects(Some("a/*")), vec!["a/point"]);
        assert!(store.list_objects(Some("b/*")).is_empty());
        assert_eq!(
            store.list_objects_of_type::<Named>(None),
            Vec::<String>::new()
        );
        assert_eq!(store.list_objects_of_type::<Point>(None), vec!["a/point"]);
        assert!(store.fetch_object::<Point>("missing").is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn blob_round_trip() {
        let (store, dir) = temp_store("blobs");
        let blob: Vec<u8> = (0..=255).collect();
        store.store_blob("flash/slot-0", &blob).unwrap();
        assert_eq!(store.fetch_blob("flash/slot-0").unwrap(), blob);
        assert_eq!(store.list_blobs(None).unwrap(), vec!["flash/slot-0"]);
        assert_eq!(store.list_blobs(Some("flash/*")).unwrap(), vec!["flash/slot-0"]);
        assert!(store.list_blobs(Some("other*")).unwrap().is_empty());
        assert!(store.fetch_blob("missing").is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn paths_are_checked() {
        let (store, dir) = temp_store("paths");
        assert!(store.store_blob("../escape", b"x").is_err());
        assert!(store.store_blob("/abs", b"x").is_err());
        assert!(store.store_blob("back\\slash", b"x").is_err());
        assert!(store.store_blob("ok/path", b"x").is_ok());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("a*", "abc"));
        assert!(wildcard_match("*c", "abc"));
        assert!(wildcard_match("a*c", "abc"));
        assert!(wildcard_match("a*c", "aXXc"));
        assert!(!wildcard_match("a*c", "abcX"));
        assert!(!wildcard_match("a", "ab"));
        assert!(wildcard_match("slot-*", "slot-3"));
        assert!(!wildcard_match("slot-*", "slotx3"));
        assert!(wildcard_match("", ""));
    }
}
