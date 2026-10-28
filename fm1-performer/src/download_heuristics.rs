//! Turning a listing entry's link into package bytes. A listing URL is rarely the
//! package itself: it may be a GitHub release *page* (resolved to its `.fwsc`
//! asset through the API), an install/flasher page (scraped for the package its
//! metadata names), or a page that names nothing at all (rejected, as is any
//! download that arrives as HTML — a stored web page boots nothing and the
//! library's status sweep would offer the same broken download forever).
//! [`crate::library`] owns the model and the threads; this module owns the
//! heuristics they run.

use std::sync::atomic::{AtomicU32, Ordering};

/// The whole pipeline: resolve the listing URL to a package URL, download it, and
/// refuse web pages (a 404's error page, a flasher naming no package). Loud
/// failure here beats a stored blob that reads Missing forever.
pub(crate) fn fetch_package(url: &str, percent: &AtomicU32) -> Result<Vec<u8>, String> {
    resolve_package_url(url).and_then(|url| {
        let bytes = download_package(&url, percent)?;
        if looks_like_web_page(&bytes[..bytes.len().min(512)]) {
            Err(format!("{url} delivered a web page, not a package"))
        } else {
            Ok(bytes)
        }
    })
}

/// A listing URL, resolved to the package itself. GitHub release pages go through
/// the API to the release's `.fwsc` asset; other page URLs (install/flasher
/// pages) are fetched and scraped for the package their metadata names; anything
/// that already looks like a package passes through unchanged.
fn resolve_package_url(url: &str) -> Result<String, String> {
    if let Some(api) = github_release_api(url) {
        return fwsc_asset_url(&fetch_text(&api)?);
    }
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".fwsc") || path.ends_with(".zip") {
        return Ok(url.to_string());
    }
    let page = fetch_text(url)?;
    install_page_package(&page)
        .and_then(|pkg| resolve_ref(url, &pkg))
        .ok_or_else(|| format!("{url} is a web page and names no package"))
}

/// The GitHub API URL for a release *page* URL
/// (`github.com/<owner>/<repo>/releases/tag/<tag>` or `…/releases/latest`), None for
/// anything else (direct package URLs included).
fn github_release_api(url: &str) -> Option<String> {
    let path = url.strip_prefix("https://github.com/")?;
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    match parts.as_slice() {
        [owner, repo, "releases", "tag", tag] => Some(format!(
            "https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}"
        )),
        [owner, repo, "releases", "latest"] => Some(format!(
            "https://api.github.com/repos/{owner}/{repo}/releases/latest"
        )),
        _ => None,
    }
}

/// The `.fwsc` asset's download URL in a GitHub release's JSON.
fn fwsc_asset_url(release_json: &str) -> Result<String, String> {
    let json: serde_json::Value =
        serde_json::from_str(release_json).map_err(|e| format!("release JSON: {e}"))?;
    json.get("assets")
        .and_then(|assets| assets.as_array())
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|asset| asset.get("browser_download_url")?.as_str())
        .find(|url| url.to_ascii_lowercase().ends_with(".fwsc"))
        .map(str::to_string)
        .ok_or_else(|| "the release has no .fwsc asset".to_string())
}

/// The fm1 install-page template carries its firmware in a metadata block —
/// `const meta = {"version": …, "pkg": "../firmware/x.fwsc", …}` — with the
/// firmware's "pkg" first (a nested "boot" object carries the bootloader's).
/// The first .fwsc-valued "pkg" is the package; the stock-firmware restore link
/// these pages also carry never uses the field.
fn install_page_package(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(at) = rest.find("\"pkg\"") {
        rest = &rest[at + "\"pkg\"".len()..];
        // The value sits between the next two quotes (`"pkg": "value"`).
        let value = rest
            .find('"')
            .and_then(|open| rest[open + 1..].find('"').map(|close| &rest[open + 1..open + 1 + close]));
        if let Some(value) = value {
            if value.to_ascii_lowercase().ends_with(".fwsc") {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Resolve a package reference found on a page against the page's URL. Absolute
/// URLs pass through; root-relative ones keep the host; the rest join the page's
/// directory with `.` and `..` honored. None when the page URL doesn't parse.
fn resolve_ref(page: &str, reference: &str) -> Option<String> {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return Some(reference.to_string());
    }
    let (scheme, rest) = page.split_once("://")?;
    let (authority, path) = match rest.find('/') {
        Some(at) => rest.split_at(at),
        None => (rest, "/"),
    };
    let root = format!("{scheme}://{authority}");
    if let Some(abs) = reference.strip_prefix('/') {
        return Some(format!("{root}/{abs}"));
    }
    let mut segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if !path.ends_with('/') {
        // The last segment is the page file itself, not a directory.
        segments.pop();
    }
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(part),
        }
    }
    Some(format!("{root}/{}", segments.join("/")))
}

/// An editor entry's download: a direct package link when one is offered, else a
/// GitHub release *page* (resolvable to its .fwsc asset through the API), the
/// install/download page, any other release page, the source repository, or the
/// first external link, in that order. The resolvable release page beats the
/// install page: the install page is an HTML flasher — never a package itself.
pub(crate) fn entry_url(links: &[(String, String)]) -> Option<String> {
    let by_label = |prefix: &str| {
        links
            .iter()
            .find(|(_, label)| label.starts_with(prefix))
            .map(|(url, _)| url.clone())
    };
    links
        .iter()
        .map(|(url, _)| url.clone())
        .find(|url| {
            let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
            path.ends_with(".fwsc") || path.ends_with(".zip")
        })
        .or_else(|| by_label("Release").filter(|url| github_release_api(url).is_some()))
        .or_else(|| by_label("Install"))
        .or_else(|| by_label("Download"))
        .or_else(|| by_label("Release"))
        .or_else(|| by_label("Source"))
        .or_else(|| links.first().map(|(url, _)| url.clone()))
}

/// Whether the head of a download looks like a web page rather than a package (a
/// listing that linked an HTML page downloaded the page itself).
pub(crate) fn looks_like_web_page(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    let text = text.trim_start();
    text.starts_with("<!doctype") || text.starts_with("<html")
}

pub(crate) fn fetch_text(url: &str) -> Result<String, String> {
    ureq::get(url)
        .call()
        .map_err(|e| format!("fetch {url}: {e}"))?
        .into_string()
        .map_err(|e| format!("read {url}: {e}"))
}

/// Download a package, reporting percent (of Content-Length) as it goes.
fn download_package(url: &str, percent: &AtomicU32) -> Result<Vec<u8>, String> {
    let response = ureq::get(url).call().map_err(|e| format!("fetch {url}: {e}"))?;
    let total: Option<u64> = response
        .header("content-length")
        .and_then(|h| h.parse().ok())
        .filter(|&t| t > 0);
    let mut body = Vec::new();
    let mut reader = response.into_reader();
    let mut chunk = [0u8; 65_536];
    loop {
        use std::io::Read;
        let read = reader.read(&mut chunk).map_err(|e| format!("read {url}: {e}"))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
        if let Some(total) = total {
            percent.store((body.len() as u64 * 100 / total).min(99) as u32, Ordering::Relaxed);
        }
    }
    percent.store(100, Ordering::Relaxed);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_release_pages_resolve_to_fwsc_assets() {
        // Tag and latest pages map onto the API; direct and foreign URLs pass through.
        assert_eq!(
            github_release_api("https://github.com/o/r/releases/tag/v1.0.0").as_deref(),
            Some("https://api.github.com/repos/o/r/releases/tags/v1.0.0")
        );
        assert_eq!(
            github_release_api("https://github.com/o/r/releases/latest").as_deref(),
            Some("https://api.github.com/repos/o/r/releases/latest")
        );
        assert_eq!(
            github_release_api("https://github.com/o/r/releases/tag/v2/?page=2#x").as_deref(),
            Some("https://api.github.com/repos/o/r/releases/tags/v2")
        );
        assert!(
            github_release_api("https://github.com/o/r/releases/download/v1/fw.fwsc").is_none()
        );
        assert!(github_release_api("https://example.com/releases/tag/v1").is_none());

        // The .fwsc asset wins; a release without one is an error, not a download.
        let json = r#"{"assets": [
            {"browser_download_url": "https://github.com/o/r/releases/download/v1/notes.txt"},
            {"browser_download_url": "https://github.com/o/r/releases/download/v1/fw.fwsc"}
        ]}"#;
        assert_eq!(
            fwsc_asset_url(json).as_deref(),
            Ok("https://github.com/o/r/releases/download/v1/fw.fwsc")
        );
        assert!(fwsc_asset_url(r#"{"assets": []}"#).is_err());
        assert!(fwsc_asset_url("not json").is_err());
    }

    #[test]
    fn entry_url_prefers_a_resolvable_release_page() {
        let links = |release: &str| {
            vec![
                (
                    "https://flash.example.com/install/".to_string(),
                    "Install ↗".to_string(),
                ),
                (release.to_string(), "Release ↗".to_string()),
                ("https://github.com/o/r".to_string(), "Source ↗".to_string()),
            ]
        };
        // A GitHub release page resolves to its .fwsc asset: it beats the flasher.
        assert_eq!(
            entry_url(&links("https://github.com/o/r/releases/tag/v1")).as_deref(),
            Some("https://github.com/o/r/releases/tag/v1")
        );
        // A release page that doesn't resolve keeps the old order: install first.
        assert_eq!(
            entry_url(&links("https://example.com/releases/v1")).as_deref(),
            Some("https://flash.example.com/install/")
        );
        // A direct package link beats everything.
        let mut with_pkg = links("https://github.com/o/r/releases/tag/v1");
        with_pkg.push((
            "https://files.example.com/x.fwsc".to_string(),
            "Mirror ↗".to_string(),
        ));
        assert_eq!(
            entry_url(&with_pkg).as_deref(),
            Some("https://files.example.com/x.fwsc")
        );
    }

    #[test]
    fn install_pages_name_their_package() {
        // The template's real shape (Album's install page): the firmware's "pkg"
        // first, the bootloader nested under "boot", a stock-firmware restore link.
        let html = r##"
        <a href="https://store.example.com/software/firmware/FM-1.fwsc">stock</a>
        <script>const meta = {"version": "0.1.20", "product": "FM-1_2701000120",
          "pkg": "../firmware/album-0.1.20.fwsc", "backup": false,
          "boot": {"pkg": "../firmware/m-bootloader-1.2.fwsc", "version": "1.2"}};</script>
        "##;
        assert_eq!(
            install_page_package(html).as_deref(),
            Some("../firmware/album-0.1.20.fwsc")
        );
        assert!(install_page_package("<p>no metadata</p>").is_none());
        // A non-package "pkg" value is skipped.
        assert!(install_page_package(r#"{"pkg": "README.md"}"#).is_none());

        let page = "https://joshbartlettband-web.github.io/album-fm1/install/";
        assert_eq!(
            resolve_ref(page, "../firmware/album-0.1.20.fwsc").as_deref(),
            Some("https://joshbartlettband-web.github.io/album-fm1/firmware/album-0.1.20.fwsc")
        );
        // A plain file joins the page's directory; a slash-less page URL drops its
        // last segment first.
        assert_eq!(
            resolve_ref(page, "pack.fwsc").as_deref(),
            Some("https://joshbartlettband-web.github.io/album-fm1/install/pack.fwsc")
        );
        assert_eq!(
            resolve_ref("https://h.test/a/install", "firmware/x.fwsc").as_deref(),
            Some("https://h.test/a/firmware/x.fwsc")
        );
        assert_eq!(
            resolve_ref(page, "/firmware/x.fwsc").as_deref(),
            Some("https://joshbartlettband-web.github.io/firmware/x.fwsc")
        );
        assert_eq!(
            resolve_ref(page, "https://cdn.test/x.fwsc").as_deref(),
            Some("https://cdn.test/x.fwsc")
        );
    }
}
