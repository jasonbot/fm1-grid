//! The small JSON feed a macOS build watches: `{ "version": "1.2.3",
//! "url": "https://…/FM1-Grid.zip" }`. Parsing and version ordering are kept
//! platform-free so they are testable on every host.

use serde::Deserialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Feed {
    pub(crate) version: String,
    pub(crate) url: String,
}

#[derive(Deserialize)]
struct Wire<'a> {
    version: &'a str,
    url: &'a str,
}

/// Parses a feed and rejects empty fields or a URL with whitespace. The URL is
/// resolved by the HTTP client, so only absolute `http(s)` URLs are accepted
/// here as well.
pub(crate) fn parse(bytes: &[u8]) -> Option<Feed> {
    let wire: Wire<'_> = serde_json::from_slice(bytes).ok()?;
    let version = wire.version.trim();
    let url = wire.url.trim();
    if version.is_empty()
        || url.is_empty()
        || url.contains(char::is_whitespace)
        || !(url.starts_with("https://") || url.starts_with("http://"))
    {
        return None;
    }
    Some(Feed {
        version: version.to_owned(),
        url: url.to_owned(),
    })
}

/// Parses the numeric core of a version. A leading `v` and a pre-release or
/// build suffix are ignored: `v1.2.3-beta.1` is `[1, 2, 3]`.
fn version_parts(version: &str) -> Option<Vec<u64>> {
    let version = version.trim().trim_start_matches(['v', 'V']);
    let core = version.split(['-', '+']).next()?;
    let parts: Option<Vec<u64>> = core
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                None
            } else {
                part.parse().ok()
            }
        })
        .collect();
    let parts = parts?;
    (!parts.is_empty()).then_some(parts)
}

/// True when `candidate` is a strictly newer version than `current`.
pub(crate) fn is_newer(candidate: &str, current: &str) -> bool {
    let (Some(candidate), Some(current)) = (version_parts(candidate), version_parts(current))
    else {
        return false;
    };
    let length = candidate.len().max(current.len());
    for index in 0..length {
        let candidate = candidate.get(index).copied().unwrap_or(0);
        let current = current.get(index).copied().unwrap_or(0);
        if candidate != current {
            return candidate > current;
        }
    }
    false
}

/// True when both sides name the same numeric version, regardless of a `v`
/// prefix or pre-release suffix.
pub(crate) fn same_version(left: &str, right: &str) -> bool {
    match (version_parts(left), version_parts(right)) {
        (Some(left), Some(right)) => {
            let length = left.len().max(right.len());
            (0..length).all(|index| {
                left.get(index).copied().unwrap_or(0) == right.get(index).copied().unwrap_or(0)
            })
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_feed() {
        let feed = parse(br#"{ "version": "v1.2.3", "url": "https://example.test/a.zip" }"#)
            .expect("feed");
        assert_eq!(feed.version, "v1.2.3");
        assert_eq!(feed.url, "https://example.test/a.zip");
    }

    #[test]
    fn rejects_bad_feeds() {
        assert!(parse(br#"{ "version": "", "url": "https://example.test/a.zip" }"#).is_none());
        assert!(parse(br#"{ "version": "1.2.3", "url": "file:///tmp/a.zip" }"#).is_none());
        assert!(
            parse(br#"{ "version": "1.2.3", "url": "https://example.test/a b.zip" }"#).is_none()
        );
        assert!(parse(br#"{ "version": "1.2.3" }"#).is_none());
    }

    #[test]
    fn orders_versions_numerically() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("v1.0", "0.9.9"));
        assert!(is_newer("1.2.3", "1.2"));
        assert!(!is_newer("1.2.0", "1.2"));
        assert!(!is_newer("1.2.3-beta.1", "1.2.3"));
        assert!(!is_newer("nonsense", "1.2.3"));
        assert!(same_version("v1.2.3-beta.1", "1.2.3"));
    }
}
