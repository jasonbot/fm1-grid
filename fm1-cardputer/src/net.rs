//! fm1-card's LAN API: UDP discovery, status, firmware listing and uploads.
//!
//! The Cardputer answers UDP `hello` on port 8135 with
//! `fm1-card;<ip>;<port>;<name>` and serves a small unauthenticated HTTP API at
//! that address. Nothing here runs on the UI thread.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::Error;

pub const DISCOVERY_PORT: u16 = 8135;
const DISCOVERY_HELLO: &[u8] = b"hello";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Discovery {
    pub name: String,
    pub host: String,
    pub addr: SocketAddr,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Status {
    pub app: String,
    pub version: String,
    pub name: String,
    pub ssid: String,
    pub rssi: i64,
    pub sd: bool,
    pub wifi: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct InstalledFirmware {
    pub name: String,
    pub dir: String,
    pub size: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct FirmwareInfo {
    pub name: String,
    pub dir: String,
    pub size: u64,
    pub sha256: String,
    pub product: String,
    pub device: String,
    pub variant: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct UploadReply {
    pub ok: bool,
    pub name: String,
    pub product: String,
    pub size: u64,
}

/// Broadcast `hello` and collect every reply for `timeout`. The source address
/// wins over the IP the payload claims, so a device behind NAT or with a stale
/// config still gets a reachable host.
pub fn discover(timeout: Duration) -> Result<Vec<Discovery>, Error> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|error| Error::new(format!("discovery socket: {error}")))?;
    socket
        .set_broadcast(true)
        .map_err(|error| Error::new(format!("discovery broadcast: {error}")))?;
    socket
        .set_read_timeout(Some(timeout))
        .map_err(|error| Error::new(format!("discovery timeout: {error}")))?;
    let targets = [
        format!("255.255.255.255:{DISCOVERY_PORT}"),
        format!("127.0.0.1:{DISCOVERY_PORT}"),
    ];
    let mut sent = false;
    for target in targets {
        if socket.send_to(DISCOVERY_HELLO, &target).is_ok() {
            sent = true;
        }
    }
    if !sent {
        return Err(Error::new("could not send the discovery broadcast"));
    }

    let deadline = Instant::now() + timeout;
    let mut found: BTreeMap<(IpAddr, u16), Discovery> = BTreeMap::new();
    let mut buffer = [0u8; 512];
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let _ = socket.set_read_timeout(Some(deadline - now));
        match socket.recv_from(&mut buffer) {
            Ok((read, from)) => {
                if let Some(mut discovery) = parse_reply(&buffer[..read], from.ip()) {
                    discovery.addr = from;
                    found.insert((from.ip(), discovery.addr.port()), discovery);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                break;
            }
            Err(_) => break,
        }
    }
    Ok(found.into_values().collect())
}

/// Parse the discovery reply. The first three fields are the original format;
/// the fourth is the editable device name.
pub fn parse_reply(payload: &[u8], from: IpAddr) -> Option<Discovery> {
    let text = std::str::from_utf8(payload).ok()?.trim();
    let mut fields = text.splitn(4, ';');
    if fields.next()? != "fm1-card" {
        return None;
    }
    let _claimed_ip = fields.next()?;
    let port: u16 = fields.next()?.parse().ok()?;
    let name = fields
        .next()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Cardputer");
    let addr = SocketAddr::new(from, port);
    Some(Discovery {
        name: name.to_string(),
        host: if port == 80 {
            from.to_string()
        } else {
            format!("{from}:{port}")
        },
        addr,
    })
}

pub fn status(host: &str) -> Result<Status, Error> {
    get_json(&format!("http://{host}/status"), Duration::from_secs(5))
}

/// Installed packages. `detail` makes the Cardputer stat every file and is
/// much slower on SD (16 s versus 0.4 s on a 64-file card), so the UI polls
/// without it and only the send path asks for hashes.
pub fn firmwares(host: &str, detail: bool) -> Result<Vec<InstalledFirmware>, Error> {
    let query = if detail { "?detail=1" } else { "" };
    let timeout = if detail {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(10)
    };
    get_json(&format!("http://{host}/firmwares{query}"), timeout)
}

pub fn firmware_info(host: &str, name: &str) -> Result<FirmwareInfo, Error> {
    get_json(
        &format!("http://{host}/firmwares/info?name={}", url_encode(name)),
        Duration::from_secs(30),
    )
}

pub fn upload(host: &str, name: &str, bytes: &[u8]) -> Result<UploadReply, Error> {
    let url = format!("http://{host}/firmwares?name={}", url_encode(name));
    let mut response = agent(Duration::from_secs(60))
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .send(bytes)
        .map_err(|error| Error::new(format!("upload {}: {error}", name)))?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| Error::new(format!("upload {}: {error}", name)))?;
    serde_json::from_str(&text)
        .map_err(|error| Error::new(format!("upload {}: bad reply: {error}", name)))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn get_json<T: serde::de::DeserializeOwned>(url: &str, timeout: Duration) -> Result<T, Error> {
    let mut response = agent(timeout)
        .get(url)
        .call()
        .map_err(|error| Error::new(format!("GET {url}: {error}")))?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| Error::new(format!("read {url}: {error}")))?;
    serde_json::from_str(&text).map_err(|error| Error::new(format!("{url}: bad JSON: {error}")))
}

fn url_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The SHA-256 of a local package, for the already-there check.
pub fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// A filename the Cardputer accepts: its API rejects path separators. The
/// display name is a safe fallback when a blob key carries no usable package
/// name.
pub fn safe_file_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').trim_matches('.');
    if out.is_empty() {
        "firmware.fwsc".to_string()
    } else if out.to_ascii_lowercase().ends_with(".fwsc") {
        out.to_string()
    } else {
        format!("{out}.fwsc")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn discovery_replies_parse_with_and_without_a_name() {
        let from = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));
        let found = parse_reply(b"fm1-card;192.168.1.20;80;Desk Card", from).unwrap();
        assert_eq!(found.name, "Desk Card");
        assert_eq!(found.host, "192.168.1.20");
        let found = parse_reply(b"fm1-card;10.0.0.2;8080", from).unwrap();
        assert_eq!(found.name, "Cardputer");
        assert_eq!(found.host, "192.168.1.20:8080");
        // The source address wins over the IP the reply names: that is the one
        // that can be reached.
        let claimed = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9));
        assert_eq!(parse_reply(b"fm1-card;10.0.0.2;80", claimed).unwrap().host, "10.0.0.9");
        assert!(parse_reply(b"not-a-card", from).is_none());
        assert!(parse_reply(b"fm1-card;1.2.3.4;not-a-port", from).is_none());
    }

    #[test]
    fn firmware_listing_uses_the_optional_detail_fields() {
        let list: Vec<InstalledFirmware> = serde_json::from_str(
            r#"[{"name":"sloop-2.8.5.fwsc","dir":"fm1-card/firmwares"},
                {"name":"x.fwsc","dir":"firmwares","size":610076,"sha256":"abc"}]"#,
        )
        .unwrap();
        assert_eq!(list[0].size, None);
        assert_eq!(list[1].sha256.as_deref(), Some("abc"));
    }

    #[test]
    fn package_names_are_safe_and_keep_their_extension() {
        assert_eq!(safe_file_name("sloop-2.8.5.fwsc"), "sloop-2.8.5.fwsc");
        assert_eq!(safe_file_name("SLOOP 2.8.5"), "SLOOP-2.8.5.fwsc");
        assert_eq!(safe_file_name("a/b"), "ab.fwsc");
        assert_eq!(safe_file_name(""), "firmware.fwsc");
    }

    #[test]
    fn names_are_percent_encoded_in_the_query() {
        assert_eq!(url_encode("a b.fwsc"), "a%20b.fwsc");
        assert_eq!(url_encode("a+b&c"), "a%2Bb%26c");
    }

    #[test]
    fn sha256_matches_a_known_digest() {
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
