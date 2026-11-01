//! One module per firmware feed: the Firmware Hub's JSON catalog (the source of
//! truth) and the additive listings — fm1-editor, Reverbose and mutant.cloud — that
//! fold into it in [`crate::library`]. Each module owns its URL constants, a
//! `fetch_listing(url)` that does the network work, and an offline-testable `parse`
//! for the body; the shared HTML scraping helpers live here.

pub(crate) mod editor;
pub(crate) mod hub;
pub(crate) mod mutant;
pub(crate) mod reverbose;

use crate::library::FirmwareDevice;

/// The slice from the end of `from` to the next `to` after it.
pub(crate) fn between<'a>(hay: &'a str, from: &str, to: &str) -> Option<&'a str> {
    let start = hay.find(from)? + from.len();
    let end = hay[start..].find(to)? + start;
    Some(&hay[start..end])
}

/// The few entities the scraped pages emit; `&amp;` last so it can't double-decode.
pub(crate) fn unescape(text: &str) -> String {
    text.replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Tags stripped, entities resolved, whitespace collapsed to single spaces.
pub(crate) fn text_of(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        rest = match rest[open..].find('>') {
            Some(close) => &rest[open + close + 1..],
            None => "",
        };
    }
    out.push_str(rest);
    unescape(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The device strings listings use ("fm-1", "smk37-pro"); unknown/empty falls back
/// to Fm1.
pub(crate) fn parse_device(product: &str) -> FirmwareDevice {
    if product.to_ascii_uppercase().starts_with("SMK") {
        FirmwareDevice::Smk37Pro
    } else {
        FirmwareDevice::Fm1
    }
}
