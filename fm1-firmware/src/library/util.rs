//! Version, date and blob-key helpers.

use super::*;

/// Piecewise numeric version compare: split on non-digits ("1.10b2" -> [1, 10, 2]).
/// True when `candidate` is newer than `current`.
pub(super) fn version_newer(candidate: &str, current: &str) -> bool {
    fn pieces(version: &str) -> Vec<u64> {
        version
            .split(|c: char| !c.is_ascii_digit())
            .filter(|piece| !piece.is_empty())
            .map(|piece| piece.parse().unwrap_or(0))
            .collect()
    }
    let candidate = pieces(candidate);
    let current = pieces(current);
    for i in 0..candidate.len().max(current.len()) {
        let a = candidate.get(i).copied().unwrap_or(0);
        let b = current.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

/// Days since the Unix epoch -> (year, month, day), Howard Hinnant's
/// civil_from_days. Valid for the whole epoch range the app cares about.
pub(super) fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// Today UTC, YYYY-MM-DD, from the system clock (no chrono).
pub(super) fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The blob-store key for a package path. Hub paths keep their file names; absolute
/// URLs (fm1-editor entries) get a hash of the whole URL, since their last segments
/// collide ("…/releases/latest" is common) or carry queries.
pub(super) fn blob_key(hub_path: &str) -> String {
    if hub_path.starts_with("http") {
        let file = hub_path
            .split(['?', '#'])
            .next()
            .and_then(|path| path.rsplit('/').next())
            .filter(|file| !file.is_empty())
            .unwrap_or("package");
        format!("ext/{}-{:016x}", file, fnv1a(hub_path))
    } else {
        format!("hub/{}", hub_path.rsplit('/').next().unwrap_or(hub_path))
    }
}

/// FNV-1a 64-bit: for blob keys only, not a checksum.
pub(super) fn fnv1a(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The package's download URL; absolute URLs pass through untouched.
pub(super) fn hub_download_url(hub_path: &str) -> String {
    if hub_path.starts_with("http") {
        hub_path.to_string()
    } else {
        format!("{}/{hub_path}", hub::BASE)
    }
}
