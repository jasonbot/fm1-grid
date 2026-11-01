//! The fm1-editor site's firmware page: a static, hand-maintained listing (its repo
//! confirms no feed exists), scraped and additive only.

use super::{between, text_of};
use crate::download_heuristics::entry_url;
use crate::library::{Firmware, FirmwareDevice, FirmwareSource, Status};

/// The firmware listing page.
pub(crate) const PAGE: &str = "https://fm1-editor.com/firmware/";

/// The status line's version: "Latest 1.4" and "Beta · Latest 1.4" carry one;
/// "No numbered releases" doesn't, and an empty version never wins a conflict.
fn status_version(status: &str) -> String {
    status
        .split_once("Latest ")
        .map(|(_, version)| version.trim().to_string())
        .unwrap_or_default()
}

/// Fetch the firmware page and parse it.
pub(crate) fn fetch_listing(url: &str) -> Result<Vec<Firmware>, String> {
    let body = crate::download_heuristics::fetch_text(url)?;
    parse_page(&body)
}

/// Parse the fm1-editor firmware page into hub-style offers. One `<article>` per
/// entry: the kicker is the author, `data-categories` the tags, the status line the
/// version, the blurb paragraphs the description, and the labelled outbound links
/// ("Source ↗", "Website ↗") the code and web pages. Entries without a name or any
/// external link are skipped. Separated from the fetch so tests stay offline.
fn parse_page(html: &str) -> Result<Vec<Firmware>, String> {
    let mut entries = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<article") {
        rest = &rest[start + "<article".len()..];
        // Closing tags are matched by their prefix only: the page wraps them across
        // lines ("</span\n>"), and the content slice ends at the marker either way.
        let end = rest.find("</article");
        let chunk = &rest[..end.unwrap_or(rest.len())];
        rest = &rest[end.unwrap_or(rest.len())..];

        let Some(name) = between(chunk, "<h2 class=\"firmware-title\"", "</h2")
            .and_then(|inner| inner.find('>').map(|i| &inner[i + 1..]))
            .map(text_of)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        let byline = between(chunk, "<p class=\"synthwave-kicker\">", "</p")
            .map(text_of)
            .unwrap_or_default();
        let version = between(chunk, "<p class=\"firmware-status\">", "</p")
            .and_then(|inner| between(inner, "<span>", "</span"))
            .map(|status| status_version(&text_of(status)))
            .unwrap_or_default();
        let tags: Vec<String> = between(chunk, "data-categories=\"", "\"")
            .map(|kinds| kinds.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default();
        let mut blurbs = Vec::new();
        let mut section = chunk;
        while let Some(open) = section.find("<p class=\"firmware-blurb\">") {
            let from = open + "<p class=\"firmware-blurb\">".len();
            let Some(close) = section[from..].find("</p") else { break };
            blurbs.push(text_of(&section[from..from + close]));
            let past = section[from + close..]
                .find('>')
                .map_or(section.len(), |i| from + close + i + 1);
            section = &section[past..];
        }
        let mut links = Vec::new();
        let mut section = chunk;
        // The page writes anchors across lines: `<a\n class=…` and `…</a\n>`.
        while let Some(anchor) = section.find("<a") {
            section = &section[anchor + 2..];
            if !section.starts_with(|c: char| c.is_ascii_whitespace()) {
                continue;
            }
            let Some(tag_end) = section.find('>') else { break };
            if let Some(href) = between(&section[..tag_end], "href=\"", "\"") {
                if href.starts_with("http") {
                    let label = section[tag_end + 1..]
                        .find("</a")
                        .map(|i| text_of(&section[tag_end + 1..tag_end + 1 + i]))
                        .unwrap_or_default();
                    links.push((href.to_string(), label));
                }
            }
            section = &section[tag_end + 1..];
        }
        let Some(url) = entry_url(&links) else { continue };
        // The page labels its outbound links: "Source ↗" names the code, "Website ↗"
        // the project's page. Both are optional.
        let labeled = |prefix: &str| {
            links
                .iter()
                .find(|(_, label)| label.to_ascii_lowercase().starts_with(prefix))
                .map(|(url, _)| url.clone())
        };
        let website = labeled("website");
        let source_url = labeled("source");

        let mut tags = tags;
        tags.push("source:fm1-editor".to_string());
        entries.push(Firmware {
            device: if name.to_ascii_uppercase().contains("SMK") {
                FirmwareDevice::Smk37Pro
            } else {
                FirmwareDevice::Fm1
            },
            name,
            byline,
            version,
            description: blurbs.join("\n\n"),
            tags,
            website,
            source_url,
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::Hub {
                path: url,
                blob: None,
            },
        });
    }
    if entries.is_empty() {
        return Err("no firmware entries found on the editor page".to_string());
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_page_parses() {
        // The page's real markup shape: one <article> per entry.
        let html = r##"
        <article class="firmware-entry synthwave-panel" id="karaoke"
                 aria-labelledby="firmware-karaoke" data-categories="grooveboxes games"
                 data-updated="2026-10-19" data-stars="4">
          <div class="firmware-body">
            <p class="synthwave-kicker">seajaysec</p>
            <h2 class="firmware-title" id="firmware-karaoke">Karaoke &amp; Robots</h2>
            <p class="firmware-status">
              <span>Beta · Latest 0.10.0</span
              ><span class="firmware-updated"
                >Updated <time datetime="2026-10-19">19 Oct 2026</time></span
              >
            </p>
            <p class="firmware-blurb">
              Sings songs you
              bring.
            </p>
            <p class="firmware-blurb">A <a href="#official-open">linked</a> second paragraph.</p>
            <ul class="firmware-links">
              <li>
                <a
                  class="firmware-anchor"
                  href="#karaoke"
                  >#</a
                >
              </li>
              <li>
                <a
                  class="hero-action firmware-link"
                  href="https://karaoke.example.com"
                  rel="noopener"
                  target="_blank"
                  >Install ↗</a
                >
              </li>
              <li>
                <a
                  class="hero-action firmware-link"
                  href="https://github.com/seajaysec/karaoke"
                  rel="noopener"
                  target="_blank"
                  >Source ↗</a
                >
              </li>
            </ul>
          </div>
        </article>
        <article class="firmware-entry" id="doom" data-categories="games" data-updated="2026-10-22">
          <div class="firmware-body">
            <p class="synthwave-kicker">id</p>
            <h2 class="firmware-title" id="firmware-doom">Doom FM-1 / SMK</h2>
            <p class="firmware-status"><span>No numbered releases</span></p>
            <p class="firmware-blurb">It runs.</p>
            <ul class="firmware-links">
              <li>
                <a
                  href="https://doom.example.com/play"
                  >Try it in a browser ↗</a
                >
              </li>
              <li>
                <a
                  href="https://files.example.com/doom.zip?raw=1"
                  >Download ↗</a
                >
              </li>
              <li>
                <a
                  href="https://doom.example.com/home"
                  >Website ↗</a
                >
              </li>
            </ul>
          </div>
        </article>
        <article class="firmware-entry" id="linkless" data-categories="tools">
          <div class="firmware-body">
            <h2 class="firmware-title" id="firmware-linkless">No Links</h2>
            <p class="firmware-status"><span>Latest 1.0</span></p>
          </div>
        </article>
        <article class="not-an-entry" data-stars="1">
          <p>No title, skipped.</p>
        </article>
        "##;
        let entries = parse_page(html).unwrap();
        assert_eq!(entries.len(), 2);
        let karaoke = &entries[0];
        assert_eq!(karaoke.name, "Karaoke & Robots");
        assert_eq!(karaoke.byline, "seajaysec");
        assert_eq!(karaoke.version, "0.10.0");
        assert_eq!(karaoke.tags, vec!["grooveboxes", "games", "source:fm1-editor"]);
        assert_eq!(karaoke.description, "Sings songs you bring.\n\nA linked second paragraph.");
        assert_eq!(karaoke.device, FirmwareDevice::Fm1);
        // The labelled links land in the link fields; karaoke has a Source but no
        // Website link.
        assert_eq!(karaoke.website, None);
        assert_eq!(
            karaoke.source_url.as_deref(),
            Some("https://github.com/seajaysec/karaoke")
        );
        match &karaoke.source {
            FirmwareSource::Hub { path, blob } => {
                assert_eq!(path, "https://karaoke.example.com");
                assert_eq!(blob, &None);
            }
            _ => panic!("hub source expected"),
        }
        let doom = &entries[1];
        assert_eq!(doom.name, "Doom FM-1 / SMK");
        assert_eq!(doom.device, FirmwareDevice::Smk37Pro);
        assert_eq!(doom.website.as_deref(), Some("https://doom.example.com/home"));
        assert_eq!(doom.source_url, None);
        // "No numbered releases" carries no version, and a direct package link beats
        // every page link.
        assert_eq!(doom.version, "");
        match &doom.source {
            FirmwareSource::Hub { path, .. } => {
                assert_eq!(path, "https://files.example.com/doom.zip?raw=1")
            }
            _ => panic!("hub source expected"),
        }
        // An entry with no external link and a non-entry article are both skipped.
        assert!(entries.iter().all(|f| f.name != "No Links"));
        assert!(parse_page("<p>nothing here</p>").is_err());
    }
}
