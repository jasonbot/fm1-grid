//! Duplicate detection and multi-source listing merge.

use super::*;

/// A name's identity key: lowercase alphanumerics only. Sources spell the same
/// firmware "PurpleMonkey", "Purple Monkey" and "PurpleMonkey FM-1"; case,
/// spaces and punctuation are spelling, not identity.
pub(super) fn name_key(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Device tokens listings tack onto firmware names, as `name_key` suffixes,
/// longest first ("SMK-37 Pro" before "SMK37", so no "pro" stump remains).
const DEVICE_SUFFIXES: [&str; 3] = ["smk37pro", "smk37", "fm1"];

/// `key` minus one trailing device token ("purplemonkeyfm1" -> "purplemonkey").
fn without_device(key: &str) -> &str {
    for suffix in DEVICE_SUFFIXES {
        if let Some(base) = key.strip_suffix(suffix) {
            if !base.is_empty() {
                return base;
            }
        }
    }
    key
}

/// Whether two entries name the same firmware. Equal name keys always match — but
/// only for the same device: most projects ship an FM-1 and an SMK-37 build under
/// one name, and merging those would lose one build and risk handing the other's
/// row the wrong package. Other spellings match when the authors agree
/// (case-insensitively) and, with device tokens stripped, one key is the other or a
/// substring of it — "Purple Monkey FM-1" and "PurpleMonkey", but not "Tuna FM-1"
/// from a different author.
pub(super) fn variant_of(firmware: &Firmware) -> Option<&str> {
    firmware.variant()
}

pub(super) fn same_firmware(a: &Firmware, b: &Firmware) -> bool {
    if a.device != b.device {
        return false;
    }
    // MKE-P37, SMK-37 Pro and SMK-37 Elite share the device enum but run different
    // packages. Never fold one model into another.
    let (va, vb) = (variant_of(a), variant_of(b));
    if (va.is_some() || vb.is_some()) && va != vb {
        return false;
    }
    let (ka, kb) = (name_key(&a.name), name_key(&b.name));
    if ka == kb {
        return true;
    }
    if !same_author(&a.byline, &b.byline) {
        return false;
    }
    let (ba, bb) = (without_device(&ka), without_device(&kb));
    ba == bb
        || (!ba.is_empty() && !bb.is_empty() && (ba.contains(bb) || bb.contains(ba)))
}

/// Author names compare case-insensitively; a blank author never identifies a row.
pub(super) fn same_author(a: &str, b: &str) -> bool {
    let a = a.trim();
    !a.is_empty() && a.to_lowercase() == b.trim().to_lowercase()
}

/// Fold an additive listing into the hub's. The hub is the source of truth: its
/// entries keep their name, author and description. A name both sources carry
/// (per [`same_firmware`]) gets the union of their non-origin tags,
/// website/source links fill gaps (the newer side's win), and the version and
/// download URL of whichever source is newer — a superseding source's `source:`
/// origin tag replaces the entry's. Names only the other source carries append at
/// the end.
pub(super) fn merge_listings(mut hub: Vec<Firmware>, editor: Vec<Firmware>) -> Vec<Firmware> {
    let mut extra = Vec::new();
    for offer in editor {
        let mut matched = false;
        for entry in hub.iter_mut() {
            if !same_firmware(entry, &offer) {
                continue;
            }
            matched = true;
            if version_newer(&offer.version, &entry.version) {
                entry.version = offer.version.clone();
                entry.source = offer.source.clone();
                // The newer side's links describe the version the URL now fetches.
                if offer.website.is_some() {
                    entry.website = offer.website.clone();
                }
                if offer.source_url.is_some() {
                    entry.source_url = offer.source_url.clone();
                }
                // The download now comes from the offer's listing: its origin tag
                // replaces the entry's.
                entry.tags.retain(|tag| !tag.starts_with("source:"));
                entry
                    .tags
                    .extend(offer.tags.iter().filter(|tag| tag.starts_with("source:")).cloned());
            }
            for tag in &offer.tags {
                // Origin tags never union in: the entry keeps its own listing's tag
                // unless the offer superseded it above.
                if tag.starts_with("source:") {
                    continue;
                }
                if !entry.tags.iter().any(|have| have.eq_ignore_ascii_case(tag)) {
                    entry.tags.push(tag.clone());
                }
            }
            // Either way, blank metadata fills in from the offer.
            if entry.description.is_empty() && !offer.description.is_empty() {
                entry.description = offer.description.clone();
            }
            if entry.byline.is_empty() && !offer.byline.is_empty() {
                entry.byline = offer.byline.clone();
            }
            if entry.website.is_none() {
                entry.website = offer.website.clone();
            }
            if entry.source_url.is_none() {
                entry.source_url = offer.source_url.clone();
            }
        }
        if !matched {
            extra.push(offer);
        }
    }
    hub.extend(extra);
    hub
}
