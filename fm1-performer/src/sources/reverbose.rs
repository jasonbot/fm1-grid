//! Reverbose's FM-1 page (reverbose.com/h/m-vave/fm-1): a server-rendered card list
//! with no feed and no API, scraped and additive only. A card names no package URL,
//! so an entry's download URL is its detail page; `package_link` follows that page
//! to the install page or release when a download actually starts.

use super::{between, text_of};
use crate::library::{Firmware, FirmwareDevice, FirmwareSource, Status};

/// The FM-1 listing page.
pub(crate) const PAGE: &str = "https://reverbose.com/h/m-vave/fm-1";

/// The host prefix detail-page URLs share.
pub(crate) const HOST: &str = "https://reverbose.com";

/// The SSR page size: the route's loader fetches 48 cards a page.
const PAGE_SIZE: usize = 48;

/// Fetch the whole listing: the server renders `?page=N` (1-based), so walk pages
/// until a short one. Fullness counts the page's cards, not the parsed entries —
/// skipped tools would fake a short page. The first page's failure is fatal; a
/// later page's just ends the walk with what's collected.
pub(crate) fn fetch_listing(page_one_url: &str) -> Result<Vec<Firmware>, String> {
    let mut all = Vec::new();
    for page in 1..=50 {
        let url = if page == 1 {
            page_one_url.to_string()
        } else {
            let sep = if page_one_url.contains('?') { '&' } else { '?' };
            format!("{page_one_url}{sep}page={page}")
        };
        let body = match crate::download_heuristics::fetch_text(&url) {
            Ok(body) => body,
            Err(e) if page == 1 => return Err(e),
            Err(_) => break,
        };
        let cards = body.matches("<article class=\"group ").count();
        match parse_page(&body) {
            Ok(entries) => all.extend(entries),
            Err(e) if page == 1 => return Err(e),
            Err(_) => break,
        }
        if cards < PAGE_SIZE {
            break;
        }
    }
    Ok(all)
}

/// Parse the listing into hub-style offers: one `<article class="group …">` card per
/// entry, carrying the name and `/f/<slug>` link, a `v`-prefixed version, an
/// `@author`, a two-line description and a kind badge. Tools (they never flash) are
/// skipped; mods get a `mod` tag. The cards carry no website/source links — those
/// live on the detail pages, which a listing fetch doesn't chase.
pub(crate) fn parse_page(html: &str) -> Result<Vec<Firmware>, String> {
    let mut entries = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<article class=\"group ") {
        rest = &rest[start + 1..];
        let end = rest.find("<article class=\"group ").unwrap_or(rest.len());
        let chunk = &rest[..end];
        rest = &rest[end..];

        // Tools run on a computer, not the hardware: nothing to flash or offer.
        if chunk.contains("title=\"Runs on your computer; nothing is flashed to the hardware.\"") {
            continue;
        }
        // The name anchor is the card's second `/f/<slug>` link — the image's has
        // no text; the name's carries the `text-[17px]` class.
        let Some(anchor) = chunk.find("<a class=\"truncate text-[17px] font-semibold") else {
            continue;
        };
        let tag = &chunk[anchor..chunk[anchor..].find('>').map_or(chunk.len(), |i| anchor + i + 1)];
        let Some(slug) = between(tag, "href=\"/f/", "\"").filter(|slug| !slug.is_empty()) else {
            continue;
        };
        let Some(name) = between(&chunk[anchor..], ">", "</a")
            .map(text_of)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        // The version span reads "v0.9.9"; entries without one stay versionless (an
        // empty version never wins a merge).
        let version = between(
            chunk,
            "ml-auto shrink-0 text-body-s tabular-nums text-text-3\">",
            "</span",
        )
        .map(|v| {
            let v = v.trim();
            v.strip_prefix('v')
                .or_else(|| v.strip_prefix('V'))
                .unwrap_or(v)
                .to_string()
        })
        .unwrap_or_default();
        let byline = between(chunk, "<span class=\"truncate\">@", "</span")
            .map(text_of)
            .unwrap_or_default();
        let description = between(
            chunk,
            "<p class=\"mt-2 line-clamp-2 min-h-[2lh] text-body-s text-text-2\">",
            "</p",
        )
        .map(text_of)
        .unwrap_or_default();
        let mut tags = Vec::new();
        // The kind badge: "Runs on top of the factory firmware." marks a mod; the
        // default ("Replaces the factory firmware.") is a full firmware.
        if chunk.contains("title=\"Runs on top of the factory firmware.\"") {
            tags.push("mod".to_string());
        }
        tags.push("source:reverbose".to_string());

        entries.push(Firmware {
            name,
            byline,
            version,
            description,
            tags,
            device: FirmwareDevice::Fm1,
            website: None,
            source_url: None,
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::Hub {
                path: format!("{HOST}/f/{slug}"),
                blob: None,
            },
        });
    }
    if entries.is_empty() {
        return Err("no firmware cards found on the reverbose page".to_string());
    }
    Ok(entries)
}

/// A detail page names no package itself: it links the project's pages. The install
/// page (the fm1 flasher, whose metadata names the package) wins, then a GitHub
/// release page, then the repository (its latest release resolves through the API).
/// Document order keeps the project's own links ahead of the credits section's.
pub(crate) fn package_link(page: &str) -> Option<String> {
    let mut hrefs = Vec::new();
    let mut rest = page;
    while let Some(at) = rest.find("href=\"") {
        rest = &rest[at + "href=\"".len()..];
        if let Some(end) = rest.find('"') {
            hrefs.push(&rest[..end]);
            rest = &rest[end..];
        }
    }
    let is_own_repo = |href: &str| {
        href.strip_prefix("https://github.com/")
            .is_some_and(|path| path.split('/').filter(|s| !s.is_empty()).count() == 2)
    };
    hrefs.iter()
        .find(|href| href.ends_with("/install/"))
        .map(|s| s.to_string())
        .or_else(|| {
            hrefs
                .iter()
                .find(|href| href.contains("github.com/") && href.contains("/releases"))
                .map(|href| match href.strip_suffix("/releases") {
                    Some(repo) => format!("{repo}/releases/latest"),
                    None => href.to_string(),
                })
        })
        .or_else(|| {
            hrefs.iter()
                .find(|href| is_own_repo(href))
                .map(|repo| format!("{repo}/releases/latest"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One card, trimmed from the live page's markup (2026-10-31).
    const CARD: &str = r#"<article class="group relative flex flex-col rounded-2xl border border-card-edge bg-surface-1 p-1.5 [container-type:inline-size] transition-[translate,box-shadow,border-color] duration-base ease-out hover:-translate-y-0.5 hover:border-border-strong hover:shadow-card">
<a class="block" tabindex="-1" aria-hidden="true" href="/f/nachtfahrt">
<div class="relative aspect-[4/3] overflow-hidden rounded-xl bg-surface-2">
<div class="isolate size-full bg-product-plate"><img src="https://raw.githubusercontent.com/FartyPants/fm1-nachtfahrt/main/docs/img/site/banner.jpg" alt="" width="1600" height="500" loading="lazy" class="size-full object-cover"/></div>
<div class="pointer-events-none absolute left-2.5 top-2.5 flex gap-1.5">
<span title="Replaces the factory firmware." class="inline-flex h-6 items-center gap-1.5 rounded-full bg-surface-1/90 px-2.5 text-[12px] font-semibold leading-none text-text-1">Firmware</span>
</div>
</div>
</a>
<div class="flex flex-1 flex-col px-2.5 pb-2 pt-3">
<div class="flex items-baseline gap-2">
<a class="truncate text-[17px] font-semibold leading-snug tracking-[-0.01em] after:absolute after:inset-0 after:rounded-2xl after:content-[&#x27;&#x27;]" href="/f/nachtfahrt">NACHTFAHRT</a>
<span class="ml-auto shrink-0 text-body-s tabular-nums text-text-3">v0.9.9</span>
</div>
<div class="mt-0.5 flex min-w-0 items-center gap-1 text-body-s text-text-3">
<a class="relative z-10 truncate font-medium text-text-2 hover:text-accent active" href="/h/m-vave/fm-1" data-status="active">FM-1</a>
<span aria-hidden="true">·</span>
<span class="truncate">@<!-- -->FartyPants</span>
<span class="ml-auto shrink-0 whitespace-nowrap pl-2 tabular-nums" title="Last release">
<time dateTime="2026-10-31T05:08:30+00:00" title="10/31/2026, 5:08:30 AM">9h</time>
</span>
</div>
<p class="mt-2 line-clamp-2 min-h-[2lh] text-body-s text-text-2">Generative sequencer instrument in the 1970s Berlin School style.</p>
</div>
</article>"#;

    #[test]
    fn reverbose_page_parses() {
        let html = format!("<html><body>{CARD}{CARD}</body></html>");
        let entries = parse_page(&html).unwrap();
        assert_eq!(entries.len(), 2);
        let entry = &entries[0];
        assert_eq!(entry.name, "NACHTFAHRT");
        assert_eq!(entry.version, "0.9.9");
        assert_eq!(entry.byline, "FartyPants");
        assert_eq!(
            entry.description,
            "Generative sequencer instrument in the 1970s Berlin School style."
        );
        assert_eq!(entry.tags, vec!["source:reverbose"]);
        assert_eq!(entry.device, FirmwareDevice::Fm1);
        assert_eq!(entry.website, None);
        match &entry.source {
            FirmwareSource::Hub { path, blob } => {
                assert_eq!(path, "https://reverbose.com/f/nachtfahrt");
                assert_eq!(blob, &None);
            }
            _ => panic!("hub source expected"),
        }
        assert!(parse_page("<p>nothing here</p>").is_err());
    }

    #[test]
    fn reverbose_skips_tools_and_tags_mods() {
        let tool = CARD.replace(
            "title=\"Replaces the factory firmware.\"",
            "title=\"Runs on your computer; nothing is flashed to the hardware.\"",
        );
        let html = format!("<html><body>{tool}</body></html>");
        assert!(parse_page(&html).is_err(), "a tools-only page yields nothing");

        let r#mod = CARD.replace(
            "title=\"Replaces the factory firmware.\"",
            "title=\"Runs on top of the factory firmware.\"",
        );
        let html = format!("<html><body>{mod}</body></html>");
        let entries = parse_page(&html).unwrap();
        assert_eq!(entries[0].tags, vec!["mod", "source:reverbose"]);
    }

    #[test]
    fn package_link_prefers_the_install_page() {
        // Trimmed from a live detail page: the project's links come before the
        // credits section's forks of other projects.
        let page = r#"<a href="https://github.com/FartyPants/fm1-nachtfahrt/stargazers">s</a>
            <a href="https://github.com/FartyPants/fm1-nachtfahrt">repo</a>
            <a href="https://fartypants.github.io/fm1-nachtfahrt/install/">Install</a>
            <a href="https://github.com/sbegueria/fm1-dub">fork credit</a>"#;
        assert_eq!(
            package_link(page).as_deref(),
            Some("https://fartypants.github.io/fm1-nachtfahrt/install/")
        );
    }

    #[test]
    fn package_link_falls_back_to_releases_then_repo() {
        let page = r#"<a href="https://github.com/o/r/releases">releases</a>"#;
        assert_eq!(
            package_link(page).as_deref(),
            Some("https://github.com/o/r/releases/latest")
        );
        let page = r#"<a href="https://github.com/o/r">repo</a>"#;
        assert_eq!(
            package_link(page).as_deref(),
            Some("https://github.com/o/r/releases/latest")
        );
        assert_eq!(package_link("<p>no links</p>"), None);
    }
}

