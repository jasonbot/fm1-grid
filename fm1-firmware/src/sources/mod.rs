//! One module per firmware feed: the Firmware Hub's JSON catalog (the source of
//! truth) and the additive listings — fm1-editor, Reverbose and mutant.cloud — that
//! fold into it in [`crate::library`]. Each module owns its URL constants, a
//! `fetch_listing(url)` that does the network work, and an offline-testable `parse`
//! for the body; the shared HTML scraping helpers live here.

pub(crate) mod editor;
pub(crate) mod hub;
pub(crate) mod jakehandy;
pub(crate) mod mutant;
pub(crate) mod reverbose;
pub(crate) mod shrub;

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


/// Chip support stated in prose: a listing that says "either/both flash chip" is
/// certified for both; one that names only the newer chip is new-chip certified.
pub(crate) fn chip_tags(text: &str) -> Vec<String> {
    let text = text.to_ascii_lowercase();
    if text.contains("either flash chip")
        || text.contains("either chip")
        || text.contains("both flash chip")
        || text.contains("both chips")
    {
        vec!["any chip".to_string()]
    } else if text.contains("newer flash chip")
        || text.contains("new flash chip")
        || text.contains("uc25hq80")
        || text.contains("0xb36014")
    {
        vec!["new chip".to_string()]
    } else {
        Vec::new()
    }
}

/// The device strings listings use ("fm-1", "smk37-pro"); unknown/empty falls back
/// to Fm1.
pub(crate) fn parse_device(product: &str) -> FirmwareDevice {
    if product.to_ascii_uppercase().starts_with("SMK") {
        FirmwareDevice::Smk37
    } else {
        FirmwareDevice::Fm1
    }
}

/// The SMK-37 model a catalog's `product` names, as a display tag: "SMK-37 Elite",
/// "SMK-37 Pro" or "MKE-P37". None for FM-1 and unknown products.
pub(crate) fn parse_variant(product: &str) -> Option<&'static str> {
    let product = product.to_ascii_uppercase();
    if product.starts_with("MKE-P37") {
        Some("MKE-P37")
    } else if product.starts_with("SMK-37 ELITE") {
        Some("SMK-37 Elite")
    } else if product.starts_with("SMK") {
        Some("SMK-37 Pro")
    } else {
        None
    }
}

/// A name made distinct per SMK-37 model: the same firmware ships one package for
/// each, and the listing often gives them one name. A name that already says the
/// model ("… (smk37-elite)") is kept. Identity keys, downloads and favorites key on
/// the name, so the suffix is what keeps the variants apart.
pub(crate) fn named_for_variant(name: String, variant: Option<&str>) -> String {
    let Some(variant) = variant else {
        return name;
    };
    let token = match variant {
        "SMK-37 Elite" => "elite",
        "SMK-37 Pro" => "pro",
        _ => "mke",
    };
    let mentions = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|part| part.eq_ignore_ascii_case(token));
    if mentions {
        name
    } else {
        format!("{name} — {variant}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_prose_maps_to_filters() {
        assert_eq!(chip_tags("works on either flash chip"), ["any chip"]);
        assert_eq!(chip_tags("supports newer FM1s with the UC25HQ80"), ["new chip"]);
        assert!(chip_tags("works on the FM-1").is_empty());
    }
}
