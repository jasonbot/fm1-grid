/// M-VAVE's SMK-37 Pro, SMK-37 Elite and MKE-P37, and the Donner Starrykey 37 Play,
/// share the board. A stock app still checks the package's model; custom firmware
/// accepts any SMK model.

pub(crate) fn is_smk37_model(model: &str) -> bool {
    let model = model.trim();
    model.to_ascii_lowercase().starts_with("smk-37")
        || model.eq_ignore_ascii_case("MKE-P37")
        || model.eq_ignore_ascii_case("STARRYKEY-37 PLAY")
}

/// Exact model match, plus the one rename M-VAVE documents: the Donner Starrykey
/// 37 Play takes the SMK-37 Pro build.
pub(crate) fn same_model(device_model: &str, package_model: &str) -> bool {
    let device_model = device_model.trim();
    let package_model = package_model.trim();
    device_model.eq_ignore_ascii_case(package_model)
        || (device_model.eq_ignore_ascii_case("STARRYKEY-37 PLAY")
            && package_model.eq_ignore_ascii_case("SMK-37 Pro"))
}

/// Whether the *running* firmware will take the package. Stock apps check their own
/// model (a Starrykey counts as an SMK-37 Pro); custom firmware (versions 7xx/9xx)
/// accepts any SMK model. FM-1 and SMK never cross.
pub fn package_fits(current: &str, product: &str) -> bool {
    let current = strip_ota(current);
    let (device_model, device_version) = current.split_once('_').unwrap_or((current, ""));
    let (package_model, _) = product.split_once('_').unwrap_or((product, ""));
    if same_model(device_model, package_model) {
        return true;
    }
    is_smk37_model(device_model)
        && is_smk37_model(package_model)
        && !device_version.starts_with('0')
}

/// The identity without an update-loader prefix.
pub(crate) fn strip_ota(text: &str) -> &str {
    text.strip_prefix("ota-")
        .or_else(|| text.strip_prefix("OTA-"))
        .unwrap_or(text)
}

/// The keyboard model an identity names, for UI labels. The Donner Starrykey
/// reports its own name but runs the SMK-37 Pro build, so it shows as SMK-37 Pro.
pub fn device_type(identity: &str) -> Option<&'static str> {
    let identity = strip_ota(identity);
    let (model, _) = identity.split_once('_').unwrap_or((identity, ""));
    if model.eq_ignore_ascii_case("FM-1") {
        Some("FM-1")
    } else if model.eq_ignore_ascii_case("SMK-37 Pro")
        || model.eq_ignore_ascii_case("STARRYKEY-37 PLAY")
    {
        Some("SMK-37 Pro")
    } else if model.eq_ignore_ascii_case("SMK-37 Elite") {
        Some("SMK-37 Elite")
    } else if model.eq_ignore_ascii_case("MKE-P37") {
        Some("MKE-P37")
    } else {
        None
    }
}

/// Whether the identity a device reports after a flash is the package's: the same
/// model family (see [`same_model`]) at the same version.
pub(crate) fn same_identity(reported: &str, product: &str) -> bool {
    let (reported_model, reported_version) =
        reported.split_once('_').unwrap_or((reported, ""));
    let (product_model, product_version) = product.split_once('_').unwrap_or((product, ""));
    // Some firmware builds report only the first few digits of their identity
    // version. The package's full version is authoritative, so a non-empty
    // reported prefix is a match. A missing version is not a prefix.
    same_model(reported_model, product_model)
        && (reported_version == product_version
            || (!reported_version.is_empty() && product_version.starts_with(reported_version)))
}

/// Pack 8-bit data into SysEx 7-bit words (a little-endian bit stream), as the update
/// protocol uses.
pub(crate) fn split_port_name(name: &str) -> (String, Option<u32>) {
    let (client, rest) = name.split_once(' ').unwrap_or((name, ""));
    let (client_name, number) = match client.split_once(':') {
        Some((client_name, number)) => (client_name, number.parse().ok()),
        None => (client, None),
    };
    let port = rest.rsplit_once(':').map(|(port, _)| port).unwrap_or(rest);
    let port = if port.is_empty() { client_name } else { port };
    (port.to_string(), number)
}

/// The device's USB bus-port path ("1-2.4"), best effort: the ALSA card whose id is
/// the port's client name, and its usbbus file. Two units with the same card id share
/// one path here; the row still tells them apart by their ALSA client number.
pub(crate) fn usb_path(port_name: &str) -> Option<String> {
    let card = port_name.split([':', ' ']).next()?;
    let cards = std::fs::read_to_string("/proc/asound/cards").ok()?;
    for line in cards.lines() {
        let Some((number, rest)) = line.trim().split_once('[') else {
            continue;
        };
        let Some(id) = rest.split(']').next() else {
            continue;
        };
        if id.trim() != card {
            continue;
        }
        let bus =
            std::fs::read_to_string(format!("/proc/asound/card{}/usbbus", number.trim())).ok()?;
        return Some(bus.trim().to_string());
    }
    None
}
