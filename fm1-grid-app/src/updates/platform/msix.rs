//! MSIX package inspection: OPC ZIP entry lookup, `AppxManifest.xml` and
//! `AppxBundleManifest.xml` identity parsing, and version ordering.
//!
//! Everything here is pure data so the Windows-only network and WinRT layers
//! stay thin and the parsing rules stay testable on every host. The updater
//! fetches only the archive tail and the manifest entry, so the helpers accept
//! a byte slice plus the absolute offset of its first byte (`base`).

use std::fmt;
use std::io::Read;

/// The manifest sits at the archive root of an MSIX.
pub(crate) const MANIFEST_NAME: &str = "AppxManifest.xml";

/// An `.msixbundle` is a ZIP whose bundle manifest lives under `AppxMetadata`.
/// Its `<Package Type="application">` entries carry the per-architecture
/// versions deployment would install.
pub(crate) const BUNDLE_MANIFEST_NAME: &str = "AppxMetadata/AppxBundleManifest.xml";

/// Deflate cannot do better than ~1000:1 on a manifest, so anything past this
/// is a malformed or hostile archive.
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const ZIP64_EOCD_SIGNATURE: u32 = 0x0606_4b50;
const ZIP64_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;

const EOCD_LEN: usize = 22;
const ZIP64_LOCATOR_LEN: usize = 20;
pub(crate) const LOCAL_HEADER_LEN: usize = 30;
const CENTRAL_HEADER_LEN: usize = 46;

/// A four-part `Package/Identity` version, ordered like `[u16; 4]`.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Version {
    pub(crate) major: u16,
    pub(crate) minor: u16,
    pub(crate) build: u16,
    pub(crate) revision: u16,
}

impl Version {
    pub(crate) const fn new(major: u16, minor: u16, build: u16, revision: u16) -> Self {
        Self {
            major,
            minor,
            build,
            revision,
        }
    }

    /// Parses a dotted identity version. All four components are required;
    /// anything else is malformed per the package schema.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.');
        let mut next = || -> Option<u16> {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        };
        let version = Self::new(next()?, next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}.{}.{}.{}",
            self.major, self.minor, self.build, self.revision
        )
    }
}

/// The central-directory record of one archive member, with offsets relative
/// to the start of the whole file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Entry {
    pub(crate) method: u16,
    pub(crate) compressed_size: u64,
    pub(crate) uncompressed_size: u64,
    pub(crate) local_header_offset: u64,
}

impl Entry {
    /// The byte range `(first, last)` of the compressed member data, once the
    /// caller has read the local header at [`Entry::local_header_offset`].
    pub(crate) fn data_range(&self, local_header: &[u8]) -> Option<(u64, u64)> {
        if local_header.len() < LOCAL_HEADER_LEN
            || read_u32(local_header, 0)? != LOCAL_SIGNATURE
            || self.compressed_size == 0
        {
            return None;
        }
        let name_len = read_u16(local_header, 26)? as u64;
        let extra_len = read_u16(local_header, 28)? as u64;
        let start = self
            .local_header_offset
            .checked_add(LOCAL_HEADER_LEN as u64)?
            .checked_add(name_len)?
            .checked_add(extra_len)?;
        Some((
            start,
            start.checked_add(self.compressed_size)?.checked_sub(1)?,
        ))
    }
}

/// A download location split into the pieces WinHTTP needs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HostedUrl {
    pub(crate) secure: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
}

impl HostedUrl {
    /// Parses an absolute `http(s)://host[:port]/path[?query]` URL. Kept
    /// deliberately small: the URL is operator-provided at build time, so
    /// credentials, fragments and non-web schemes are rejected rather than
    /// normalized.
    pub(crate) fn parse(url: &str) -> Option<Self> {
        let url = url.trim();
        let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
            (true, rest)
        } else {
            (false, url.strip_prefix("http://")?)
        };
        let rest = rest.split('#').next()?;
        let (authority, path) = match rest.find('/') {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, "/"),
        };
        if authority.is_empty()
            || authority.contains(['@', '?', '#'])
            || path.contains(char::is_whitespace)
        {
            return None;
        }

        let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
            let (host, rest) = rest.split_once(']')?;
            (
                host,
                rest.strip_prefix(':')
                    .map(str::parse::<u16>)
                    .transpose()
                    .ok()?,
            )
        } else {
            match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port.parse::<u16>().ok()?)),
                None => (authority, None),
            }
        };
        if host.is_empty() || host.contains(char::is_whitespace) {
            return None;
        }

        Some(Self {
            secure,
            host: host.to_owned(),
            port: port.unwrap_or(if secure { 443 } else { 80 }),
            path: path.to_owned(),
        })
    }
}

/// Finds `name` in the archive's central directory. `base` is the absolute
/// offset of `bytes[0]`; the whole directory must be contained in `bytes`.
pub(crate) fn find_entry(bytes: &[u8], base: u64, name: &str) -> Option<Entry> {
    let eocd = find_eocd(bytes)?;
    let (count, directory_size, directory_offset) = directory_fields(bytes, base, eocd)?;
    let count = usize::try_from(count).ok()?;
    let directory_start = usize::try_from(directory_offset.checked_sub(base)?).ok()?;
    let directory_end = directory_start.checked_add(usize::try_from(directory_size).ok()?)?;
    if directory_end > bytes.len() {
        return None;
    }

    let mut cursor = directory_start;
    for _ in 0..count {
        if read_u32(bytes, cursor)? != CENTRAL_SIGNATURE {
            return None;
        }
        let method = read_u16(bytes, cursor + 10)?;
        let compressed_size = read_u32(bytes, cursor + 20)? as u64;
        let uncompressed_size = read_u32(bytes, cursor + 24)? as u64;
        let name_len = read_u16(bytes, cursor + 28)? as usize;
        let extra_len = read_u16(bytes, cursor + 30)? as usize;
        let comment_len = read_u16(bytes, cursor + 32)? as usize;
        let local_header_offset = read_u32(bytes, cursor + 42)? as u64;
        let name_start = cursor + CENTRAL_HEADER_LEN;
        let name_end = name_start.checked_add(name_len)?;
        let entry_name = bytes.get(name_start..name_end)?;
        if entry_name.eq_ignore_ascii_case(name.as_bytes()) {
            return Some(Entry {
                method,
                compressed_size,
                uncompressed_size,
                local_header_offset,
            });
        }
        cursor = name_end.checked_add(extra_len)?.checked_add(comment_len)?;
    }
    None
}

/// Reads a central-directory member from a fully buffered archive.
pub(crate) fn read_entry(archive: &[u8], entry: Entry) -> Option<Vec<u8>> {
    let local = usize::try_from(entry.local_header_offset).ok()?;
    let local_header = archive.get(local..local.checked_add(LOCAL_HEADER_LEN)?)?;
    let (start, last) = entry.data_range(local_header)?;
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(last.checked_add(1)?).ok()?;
    read_compressed(entry, archive.get(start..end)?)
}

/// Which manifest an archive carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Manifest {
    /// An `.msixbundle`: each `<Package Type="application">` entry names the
    /// version deployment would install for that architecture.
    Bundle(Entry),
    /// A plain `.msix`: the `<Identity>` version is the package version.
    Package(Entry),
}

impl Manifest {
    pub(crate) fn entry(&self) -> Entry {
        match *self {
            Self::Bundle(entry) | Self::Package(entry) => entry,
        }
    }

    /// The version this archive would install on `architecture`.
    pub(crate) fn version(&self, manifest: &[u8], architecture: &str) -> Option<Version> {
        match self {
            Self::Bundle(_) => bundle_version(manifest, architecture),
            Self::Package(_) => identity_version(manifest),
        }
    }
}

/// Finds the manifest entry in a tail: a bundle's `AppxBundleManifest.xml`
/// first, then a package's `AppxManifest.xml`. A bundle has no manifest at its
/// root, and a plain MSIX has no `AppxMetadata` entry, so the order is only
/// defensive.
pub(crate) fn find_manifest_entry(bytes: &[u8], base: u64) -> Option<Manifest> {
    if let Some(entry) = find_entry(bytes, base, BUNDLE_MANIFEST_NAME) {
        return Some(Manifest::Bundle(entry));
    }
    find_entry(bytes, base, MANIFEST_NAME).map(Manifest::Package)
}

/// The version a fully buffered `.msix` or `.msixbundle` would install on
/// `architecture`.
pub(crate) fn package_version(archive: &[u8], architecture: &str) -> Option<Version> {
    let manifest = find_manifest_entry(archive, 0)?;
    manifest.version(&read_entry(archive, manifest.entry())?, architecture)
}

/// The `Version` of the bundle's application package for `architecture`, or of
/// its neutral application package when there is no architecture match.
///
/// The `<Package>` entries are authoritative here, not the bundle
/// `<Identity>`: Store bundles number the bundle independently of the packages
/// inside (for example `3001.*` versus `1.*`), so comparing an installed
/// package version against the bundle identity would misreport every build.
pub(crate) fn bundle_version(manifest: &[u8], architecture: &str) -> Option<Version> {
    let manifest = manifest.strip_prefix(b"\xef\xbb\xbf").unwrap_or(manifest);
    let text = std::str::from_utf8(manifest).ok()?;

    let mut rest = text;
    let mut neutral = None;
    while let Some(index) = rest.find("<Package") {
        let after = &rest[index + "<Package".len()..];
        if !after.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            rest = after;
            continue;
        }
        let Some(tag_end) = after.find('>') else {
            break;
        };
        let tag = &after[..tag_end];
        rest = &after[tag_end..];

        if !attribute_value(tag, "Type")
            .is_some_and(|kind| kind.eq_ignore_ascii_case("application"))
        {
            continue;
        }
        let Some(version) = attribute(tag, "Version") else {
            continue;
        };
        let package_arch = attribute_value(tag, "Architecture").unwrap_or("neutral");
        if package_arch.eq_ignore_ascii_case(architecture) {
            return Some(version);
        }
        if package_arch.eq_ignore_ascii_case("neutral") {
            neutral = Some(version);
        }
    }
    neutral
}

/// Extracts `Version` from the `<Identity ...>` element of a manifest.
pub(crate) fn identity_version(manifest: &[u8]) -> Option<Version> {
    let manifest = manifest.strip_prefix(b"\xef\xbb\xbf").unwrap_or(manifest);
    let text = std::str::from_utf8(manifest).ok()?;

    let mut rest = text;
    while let Some(index) = rest.find("<Identity") {
        let after_name = &rest[index + "<Identity".len()..];
        if !after_name.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/') {
            rest = after_name;
            continue;
        }
        let tag_end = after_name.find('>')?;
        return attribute(&after_name[..tag_end], "Version");
    }
    None
}

/// Parses a `bytes <start>-<end>/<total>` response header into its parts.
pub(crate) fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    let value = value.strip_prefix("bytes")?.trim_start();
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    let start = start.trim().parse().ok()?;
    let end = end.trim().parse().ok()?;
    let total = total.trim().parse().ok()?;
    (start <= end && end < total).then_some((start, end, total))
}

/// Decodes a member's compressed bytes, enforcing the manifest size cap.
pub(crate) fn read_compressed(entry: Entry, compressed: &[u8]) -> Option<Vec<u8>> {
    let expected = usize::try_from(entry.uncompressed_size).ok()?;
    if expected > MAX_MANIFEST_BYTES || compressed.len() as u64 != entry.compressed_size {
        return None;
    }
    let mut data = Vec::with_capacity(expected.min(64 * 1024));
    match entry.method {
        0 => data.extend_from_slice(compressed),
        8 => {
            let limit = MAX_MANIFEST_BYTES as u64 + 1;
            flate2::read::DeflateDecoder::new(compressed)
                .take(limit)
                .read_to_end(&mut data)
                .ok()?;
        }
        _ => return None,
    }
    (data.len() == expected).then_some(data)
}

fn find_eocd(bytes: &[u8]) -> Option<usize> {
    let last = bytes.len().checked_sub(EOCD_LEN)?;
    let first = last.saturating_sub(u16::MAX as usize);
    (first..=last)
        .rev()
        .find(|&index| read_u32(bytes, index) == Some(EOCD_SIGNATURE))
}

/// Central-directory `(entries, size, offset)` from the end-of-central-directory
/// records. MakeAppx writes ZIP64 records even for small packages, so a
/// standard record whose fields are `0xFFFF`/`0xFFFFFFFF` sentinels must be
/// resolved through the immediately preceding ZIP64 locator and record.
fn directory_fields(bytes: &[u8], base: u64, eocd: usize) -> Option<(u64, u64, u64)> {
    let count = read_u16(bytes, eocd + 10)? as u64;
    let size = read_u32(bytes, eocd + 12)? as u64;
    let offset = read_u32(bytes, eocd + 16)? as u64;
    if count != u16::MAX as u64 && size != u32::MAX as u64 && offset != u32::MAX as u64 {
        return Some((count, size, offset));
    }

    let locator = eocd.checked_sub(ZIP64_LOCATOR_LEN)?;
    if read_u32(bytes, locator)? != ZIP64_LOCATOR_SIGNATURE {
        return None;
    }
    let record = usize::try_from(read_u64(bytes, locator + 8)?.checked_sub(base)?).ok()?;
    if read_u32(bytes, record)? != ZIP64_EOCD_SIGNATURE {
        return None;
    }
    Some((
        read_u64(bytes, record + 32)?,
        read_u64(bytes, record + 40)?,
        read_u64(bytes, record + 48)?,
    ))
}

fn attribute(tag: &str, name: &str) -> Option<Version> {
    Version::parse(attribute_value(tag, name)?)
}

/// The raw value of attribute `name` inside a start tag (without its quotes).
fn attribute_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut search = 0;
    while let Some(index) = tag[search..].find(name) {
        let start = search + index;
        let boundary = start == 0 || tag.as_bytes()[start - 1].is_ascii_whitespace();
        let after = tag[start + name.len()..].trim_start();
        if boundary {
            if let Some(value) = after.strip_prefix('=') {
                let value = value.trim_start();
                let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                let end = value[1..].find(quote)?;
                return Some(&value[1..1 + end]);
            }
        }
        search = start + name.len();
    }
    None
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn manifest(version: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10">
  <Identity Name="FM1.Grid" Publisher="CN=FM1" Version="{version}" ProcessorArchitecture="x64" />
  <Properties><DisplayName>FM1 Grid</DisplayName></Properties>
</Package>"#
        )
        .into_bytes()
    }

    /// The real shape MakeAppx emits for a multi-architecture bundle.
    fn bundle_manifest(identity_version: &str, package_version: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<Bundle SchemaVersion="2.0" xmlns="http://schemas.microsoft.com/appx/2013/bundle">
  <Identity Name="FM1.Grid" Publisher="CN=FM1" Version="{identity_version}"/>
  <Packages>
    <Package Type="application" Version="{package_version}" Architecture="x64" FileName="FM1.Grid_{package_version}_x64.msix"/>
    <Package Type="application" Version="{package_version}" Architecture="arm64" FileName="FM1.Grid_{package_version}_arm64.msix"/>
  </Packages>
</Bundle>"#
        )
        .into_bytes()
    }

    /// Minimal ZIP writer: one stored and one deflated member plus the central
    /// directory, enough to exercise the reader without a zip dependency.
    fn archive(members: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut directory = Vec::new();
        for &(name, data, deflate) in members {
            let offset = out.len() as u32;
            let payload = if deflate {
                let mut encoder =
                    flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                encoder.write_all(data).unwrap();
                encoder.finish().unwrap()
            } else {
                data.to_vec()
            };
            let method: u16 = if deflate { 8 } else { 0 };
            let mut local = Vec::with_capacity(LOCAL_HEADER_LEN + name.len());
            local.extend_from_slice(&LOCAL_SIGNATURE.to_le_bytes());
            local.extend_from_slice(&[0; 4]); // version needed, flags
            local.extend_from_slice(&method.to_le_bytes());
            local.extend_from_slice(&[0; 4]); // time, date
            local.extend_from_slice(&[0; 4]); // crc
            local.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            local.extend_from_slice(&(data.len() as u32).to_le_bytes());
            local.extend_from_slice(&(name.len() as u16).to_le_bytes());
            local.extend_from_slice(&0u16.to_le_bytes());
            local.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&local);
            out.extend_from_slice(&payload);

            directory.extend_from_slice(&CENTRAL_SIGNATURE.to_le_bytes());
            directory.extend_from_slice(&[0; 4]); // versions
            directory.extend_from_slice(&[0; 2]); // flags
            directory.extend_from_slice(&method.to_le_bytes());
            directory.extend_from_slice(&[0; 8]); // time, date, crc
            directory.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
            directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
            directory.extend_from_slice(&[0; 8]); // extra, comment, disk, internal
            directory.extend_from_slice(&[0; 4]); // external attributes
            directory.extend_from_slice(&offset.to_le_bytes());
            directory.extend_from_slice(name.as_bytes());
        }
        let directory_offset = out.len() as u32;
        out.extend_from_slice(&directory);
        out.extend_from_slice(&EOCD_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&[0; 4]); // disk, central-directory disk
        out.extend_from_slice(&(members.len() as u16).to_le_bytes()); // entries on disk
        out.extend_from_slice(&(members.len() as u16).to_le_bytes()); // total entries
        out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
        out.extend_from_slice(&directory_offset.to_le_bytes());
        out.extend_from_slice(&[0; 2]);
        out
    }

    /// Rewrites a fixture's trailer the way MakeAppx does: a ZIP64 EOCD record
    /// and locator, then the standard EOCD with sentinel fields.
    fn zip64_archive(members: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let mut out = archive(members);
        let eocd = out.len() - EOCD_LEN;
        let count = read_u16(&out, eocd + 10).unwrap() as u64;
        let size = read_u32(&out, eocd + 12).unwrap() as u64;
        let offset = read_u32(&out, eocd + 16).unwrap() as u64;
        out.truncate(eocd);

        let record = out.len() as u64;
        out.extend_from_slice(&ZIP64_EOCD_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&44u64.to_le_bytes()); // remaining record size
        out.extend_from_slice(&45u16.to_le_bytes()); // version made by
        out.extend_from_slice(&45u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u32.to_le_bytes()); // disk
        out.extend_from_slice(&0u32.to_le_bytes()); // central-directory disk
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&count.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());

        out.extend_from_slice(&ZIP64_LOCATOR_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // disk
        out.extend_from_slice(&record.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // disks

        out.extend_from_slice(&EOCD_SIGNATURE.to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&u16::MAX.to_le_bytes());
        out.extend_from_slice(&u16::MAX.to_le_bytes());
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(&[0; 2]);
        out
    }

    #[test]
    fn version_order_and_display() {
        let old = Version::parse("1.2.3.4").unwrap();
        let new = Version::parse("1.2.4.0").unwrap();
        assert!(new > old);
        assert_eq!(new.to_string(), "1.2.4.0");
        assert_eq!(Version::parse("65535.0.0.65535").unwrap().revision, 65535);
        assert_eq!(Version::parse("1.1.1"), None);
        assert_eq!(Version::parse("1.1.1.1.1"), None);
        assert_eq!(Version::parse("1.1.1.x"), None);
        assert_eq!(Version::parse("1..1.1"), None);
        assert_eq!(Version::parse(""), None);
    }

    #[test]
    fn identity_version_reads_the_attribute() {
        assert_eq!(
            identity_version(&manifest("2.10.0.7")).unwrap(),
            Version::new(2, 10, 0, 7)
        );
        assert_eq!(identity_version(b"<Package/>"), None);
        // A different element must not shadow the identity.
        assert_eq!(
            identity_version(
                b"<Properties Version=\"9.9.9.9\"><Identity Version='3.4.5.6'/></Properties>"
            )
            .unwrap(),
            Version::new(3, 4, 5, 6)
        );
    }

    #[test]
    fn package_version_reads_deflated_and_stored_members() {
        for deflate in [false, true] {
            let zip = archive(&[
                ("[Content_Types].xml", b"<Types/>", true),
                (MANIFEST_NAME, &manifest("7.8.9.10"), deflate),
            ]);
            assert_eq!(
                package_version(&zip, "x64"),
                Some(Version::new(7, 8, 9, 10))
            );
        }
    }

    #[test]
    fn package_version_reads_bundle_manifests() {
        for deflate in [false, true] {
            let zip = archive(&[
                ("[Content_Types].xml", b"<Types/>", true),
                (
                    BUNDLE_MANIFEST_NAME,
                    &bundle_manifest("2.4.6.8", "2.4.6.8"),
                    deflate,
                ),
                ("FM1.Grid_2.4.6.8_x64.msix", b"inner", false),
                ("FM1.Grid_2.4.6.8_arm64.msix", b"inner", false),
            ]);
            for architecture in ["x64", "arm64"] {
                assert_eq!(
                    package_version(&zip, architecture),
                    Some(Version::new(2, 4, 6, 8)),
                    "{architecture}"
                );
            }
            // A bundle without the running architecture installs nothing.
            assert_eq!(package_version(&zip, "x86"), None);
        }
    }

    #[test]
    fn bundle_version_prefers_package_entries_over_the_bundle_identity() {
        // Store bundles number the bundle independently of the packages, so the
        // installed package must be compared against its own entry.
        let manifest = bundle_manifest("3001.1.0.0", "1.1.0.0");
        assert_eq!(
            bundle_version(&manifest, "x64"),
            Some(Version::new(1, 1, 0, 0))
        );
        assert_eq!(
            bundle_version(&manifest, "arm64"),
            Some(Version::new(1, 1, 0, 0))
        );
        assert_eq!(bundle_version(&manifest, "x86"), None);
    }

    #[test]
    fn bundle_version_falls_back_to_a_neutral_package() {
        let manifest = br#"<Bundle><Packages>
  <Package Type="application" Version="4.5.6.7" FileName="app.msix"/>
  <Package Type="resource" Version="9.9.9.9" Architecture="x64" FileName="res.msix"/>
</Packages></Bundle>"#;
        assert_eq!(
            bundle_version(manifest, "x64"),
            Some(Version::new(4, 5, 6, 7))
        );
    }

    #[test]
    fn bundle_manifest_wins_over_a_root_package_manifest() {
        let zip = archive(&[
            (MANIFEST_NAME, &manifest("1.0.0.0"), true),
            (
                BUNDLE_MANIFEST_NAME,
                &bundle_manifest("2.0.0.0", "2.0.0.0"),
                true,
            ),
        ]);
        assert_eq!(package_version(&zip, "x64"), Some(Version::new(2, 0, 0, 0)));
    }

    #[test]
    fn bundle_manifest_offsets_work_from_a_tail() {
        let zip = archive(&[
            ("padding.bin", &[0x55; 4096], false),
            (
                BUNDLE_MANIFEST_NAME,
                &bundle_manifest("3.1.4.1", "3.1.4.1"),
                true,
            ),
            ("FM1.Grid_3.1.4.1_x64.msix", &[0; 1024], false),
        ]);
        let base = 4096;
        let tail = &zip[base..];
        let manifest = find_manifest_entry(tail, base as u64).unwrap();
        let entry = manifest.entry();
        let local = &zip[entry.local_header_offset as usize..];
        let (start, last) = entry.data_range(&local[..LOCAL_HEADER_LEN]).unwrap();
        let data = &zip[start as usize..last as usize + 1];
        let decoded = read_compressed(entry, data).unwrap();
        assert_eq!(
            manifest.version(&decoded, "arm64"),
            Some(Version::new(3, 1, 4, 1))
        );
    }

    #[test]
    fn zip64_trailers_resolve_the_directory() {
        // Both a bundle and a plain package, because MakeAppx emits the ZIP64
        // trailer for either.
        let bundle = zip64_archive(&[
            ("padding.bin", &[0x55; 4096], false),
            (
                BUNDLE_MANIFEST_NAME,
                &bundle_manifest("5.6.7.8", "5.6.7.8"),
                true,
            ),
        ]);
        assert_eq!(
            package_version(&bundle, "x64"),
            Some(Version::new(5, 6, 7, 8))
        );

        let package = zip64_archive(&[
            ("padding.bin", &[0x55; 4096], false),
            (MANIFEST_NAME, &manifest("6.0.0.1"), true),
        ]);
        assert_eq!(
            package_version(&package, "x64"),
            Some(Version::new(6, 0, 0, 1))
        );

        // The tail path must resolve the locator's absolute record offset.
        let base = 4096;
        let tail = &bundle[base..];
        let found = find_manifest_entry(tail, base as u64).unwrap();
        let entry = found.entry();
        let local = &bundle[entry.local_header_offset as usize..];
        let (start, last) = entry.data_range(&local[..LOCAL_HEADER_LEN]).unwrap();
        let decoded = read_compressed(entry, &bundle[start as usize..last as usize + 1]).unwrap();
        assert_eq!(
            found.version(&decoded, "arm64"),
            Some(Version::new(5, 6, 7, 8))
        );
    }

    #[test]
    fn central_directory_offsets_work_from_a_tail() {
        let zip = archive(&[
            ("padding.bin", &[0x55; 4096], false),
            (MANIFEST_NAME, &manifest("1.0.0.2"), true),
        ]);
        let base = 4096;
        let tail = &zip[base..];
        let entry = find_entry(tail, base as u64, MANIFEST_NAME).unwrap();
        // The manifest data is beyond the tail; fetch the local header and its
        // byte range exactly like the updater does over HTTP.
        let local = &zip[entry.local_header_offset as usize..];
        let (start, last) = entry.data_range(&local[..LOCAL_HEADER_LEN]).unwrap();
        assert!(start as usize >= base);
        let data = &zip[start as usize..last as usize + 1];
        let decoded = read_compressed(entry, data).unwrap();
        assert_eq!(identity_version(&decoded), Some(Version::new(1, 0, 0, 2)));
    }

    #[test]
    fn malformed_members_are_rejected() {
        let zip = archive(&[(MANIFEST_NAME, &manifest("1.0.0.1"), true)]);
        let entry = find_entry(&zip, 0, MANIFEST_NAME).unwrap();
        assert_eq!(
            read_compressed(
                Entry {
                    method: 14,
                    ..entry
                },
                &[0; 1]
            ),
            None
        );
        assert_eq!(find_entry(&zip[..10], 0, MANIFEST_NAME), None);
        assert_eq!(read_entry(&zip[..10], entry), None);
    }

    #[test]
    fn hosted_url_parsing() {
        assert_eq!(
            HostedUrl::parse("https://cdn.example.com/a/b.msix?x=1").unwrap(),
            HostedUrl {
                secure: true,
                host: "cdn.example.com".into(),
                port: 443,
                path: "/a/b.msix?x=1".into(),
            }
        );
        assert_eq!(
            HostedUrl::parse("http://example.com:8080").unwrap().path,
            "/"
        );
        assert_eq!(
            HostedUrl::parse("https://[2001:db8::1]:8443/x").unwrap(),
            HostedUrl {
                secure: true,
                host: "2001:db8::1".into(),
                port: 8443,
                path: "/x".into(),
            }
        );
        for bad in [
            "ftp://example.com/x",
            "example.com/x",
            "https:///x",
            "https://user@example.com/x",
            "https://example.com?x=1",
        ] {
            assert_eq!(HostedUrl::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn content_range_parsing() {
        assert_eq!(
            parse_content_range("bytes 100-199/200"),
            Some((100, 199, 200))
        );
        assert_eq!(parse_content_range("bytes 0-1/2"), Some((0, 1, 2)));
        assert_eq!(parse_content_range("bytes 0-1/2\u{0}"), Some((0, 1, 2)));
        assert_eq!(parse_content_range("bytes */200"), None);
        assert_eq!(parse_content_range("bytes 200-100/300"), None);
        assert_eq!(parse_content_range("garbage"), None);
    }
}
