//! MSIX package inspection: OPC ZIP entry lookup, `AppxManifest.xml` identity
//! parsing and version ordering.
//!
//! Everything here is pure data so the Windows-only network and WinRT layers
//! stay thin and the parsing rules stay testable on every host. The updater
//! fetches only the archive tail and the manifest entry, so the helpers accept
//! a byte slice plus the absolute offset of its first byte (`base`).

use std::fmt;
use std::io::Read;

/// The manifest sits at the archive root of an MSIX.
pub(crate) const MANIFEST_NAME: &str = "AppxManifest.xml";

/// Deflate cannot do better than ~1000:1 on a manifest, so anything past this
/// is a malformed or hostile archive.
const MAX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;

const EOCD_LEN: usize = 22;
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
    let count = read_u16(bytes, eocd + 10)? as usize;
    let directory_size = read_u32(bytes, eocd + 12)? as u64;
    let directory_offset = read_u32(bytes, eocd + 16)? as u64;
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

/// The `Version` from the archive's `AppxManifest.xml`, if it has one.
pub(crate) fn manifest_version(archive: &[u8]) -> Option<Version> {
    let entry = find_entry(archive, 0, MANIFEST_NAME)?;
    identity_version(&read_entry(archive, entry)?)
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

fn attribute(tag: &str, name: &str) -> Option<Version> {
    let mut search = 0;
    while let Some(index) = tag[search..].find(name) {
        let start = search + index;
        let boundary = start == 0 || tag.as_bytes()[start - 1].is_ascii_whitespace();
        let after = &tag[start + name.len()..];
        let after = after.trim_start();
        if boundary {
            if let Some(value) = after.strip_prefix('=') {
                let value = value.trim_start();
                let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
                let end = value[1..].find(quote)?;
                return Version::parse(&value[1..1 + end]);
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
    fn manifest_version_reads_deflated_and_stored_members() {
        for deflate in [false, true] {
            let zip = archive(&[
                ("[Content_Types].xml", b"<Types/>", true),
                (MANIFEST_NAME, &manifest("7.8.9.10"), deflate),
            ]);
            assert_eq!(manifest_version(&zip), Some(Version::new(7, 8, 9, 10)));
        }
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
