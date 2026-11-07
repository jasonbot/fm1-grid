/// Known product strings and the firmwares that report them (thefirmwarehub.com's
/// catalogue, October 2026). Several projects share one product string; a single exact
/// hit gets a friendly name, a shared one shows how many firmwares it could be.
pub(crate) const KNOWN: &[(&str, &[&str])] = &[
    ("FM-1_003", &["Bubba Machine"]),
    ("FM-1_1500", &["ETCH"]),
    ("FM-1_2123", &["BEETLE"]),
    ("FM-1_337", &["Chipbox"]),
    ("FM-1_600", &["FM1-GM"]),
    ("FM-1_700", &["Tonitor", "Vector37"]),
    ("FM-1_703", &["Fart Machine One"]),
    ("FM-1_714", &["Skiff"]),
    ("FM-1_777", &["Evander's Music Box"]),
    ("FM-1_80023", &["Unholy Union"]),
    ("FM-1_900", &[
        "Hortator",
        "GHOULBOX",
        "SLOOP",
        "Clipper",
        "JIANT FM",
        "winampFM Synth",
        "MYCELIUM",
        "ORBIT",
        "Felucca 8 Patterns",
        "PIGEON",
        "NOIZ DEMONS",
        "MC-1",
        "Tuna",
        "No Man Sky FM",
        "FM1 Quest",
        "Mini-Tone",
        "FMini-1",
        "Golana DZ Edition",
        "Flyfish",
    ]),
    ("FM-1_901", &["DUELL"]),
    ("FM-1_903", &["fm1-chord"]),
    ("FM-1_907", &["FM-106"]),
    ("FM-1_909", &["Felucca [Salt]"]),
    ("FM-1_910", &["Jangada", "WaveLoop FM-1", "MAKU"]),
    ("FM-1_91045", &["Melodee"]),
    ("FM-1_911", &["Rainbow Mode"]),
    ("FM-1_912", &["Golana"]),
    ("FM-1_916", &["Felucca"]),
    ("FM-1_920", &["ChoralRoot"]),
    ("FM-1_924", &["SLOOP Floyd FM"]),
    ("FM-1_925", &["SLOOP DX7 Banks"]),
    ("FM-1_927", &["PurpleMonkey"]),
    ("FM-1_930", &["CoralFlower"]),
    ("FM-1_940", &["sloopDX"]),
    ("FM-1_942", &["Bytemania"]),
    ("FM-1_950", &["CS-1", "OPN-1", "FMEL-1"]),
    ("FM-1_951", &["Ketch"]),
    ("FM-1_955", &["OberVave", "KAWA"]),
    ("FM-1_958", &["Frodhes"]),
    ("FM-1_960", &["SID-1"]),
    ("FM-1_961", &["FOMAJAM"]),
    ("FM-1_962", &["Shine On"]),
    ("FM-1_966", &["GR!MM"]),
    ("FM-1_974", &["zp12"]),
    ("FM-1_981", &["Pocket GammaWave"]),
    ("FM-1_993", &["Dinghy"]),
    ("FM-1_3000500", &["Polly"]),
    ("FM-1_386010002", &["386DX"]),
    ("FM-1_42000300", &["MODELAR"]),
    ("FM-1_4310100", &["HERA"]),
    ("FM-1_5000201", &["MIN-1"]),
    ("FM-1_5000901", &["MONK-1"]),
    ("FM-1_6010000", &["CTL-1"]),
    ("FM-1_6010200", &["ZEN-1"]),
    ("FM-1_7010100", &["FuMi-1"]),
    ("FM-1_7010201", &["Cut-A-Dor"]),
    ("FM-1_7010300", &["AMB-1"]),
    ("FM-1_7010554", &["HYP-1"]),
    ("FM-1_7100002", &["Bunker Evolved"]),
    ("FM-1_710010000", &["Mutable Dreams"]),
    ("FM-1_8000013", &["MORPHEUS"]),
    ("FM-1_8000200", &["Niagara Drift"]),
    ("FM-1_8000400", &["FoMni"]),
    ("FM-1_8000601", &["FiMba-1"]),
    ("FM-1_8010300", &["DUB-1"]),
    ("FM-1_9000300", &["TRIBE"]),
    ("FM-1_9010100", &["X0X"]),
    ("FM-1_987010100", &["X0X-RMX"]),
    ("FM-1_10000601", &["LOOM-8"]),
];

/// Older firmware versions report other numbers: (prefix, digits after it, family
/// name), checked only when no exact product matches (the catalogue's ident patterns).
pub(crate) const FAMILIES: &[(&str, usize, &str)] = &[
    ("FM-1_386", 6, "386DX"),
    ("FM-1_800", 4, "FoMni or FiMba-1"),
    ("FM-1_90", 5, "X0X"),
    ("FM-1_70", 5, "FuMi-1"),
    ("FM-1_97", 1, "zp12"),
    ("FM-1_93", 1, "sloopDX"),
    ("FM-1_95", 1, "OberVave or KAWA"),
    ("FM-1_99", 1, "Dinghy"),
    ("FM-1_91", 1, "Felucca"),
    ("FM-1_71", 1, "Skiff"),
    ("FM-1_3", 6, "Polly"),
];

/// A friendly name for an identity text: "Felucca (FM-1_916)", "the update loader
/// (ota-FM-1_916)", "the official firmware V15", or the raw text when unknown.
pub(crate) fn friendly(text: &str) -> String {
    if let Some(rest) = text
        .strip_prefix("ota-")
        .or_else(|| text.strip_prefix("OTA-"))
    {
        return format!("the update loader ({rest})");
    }
    match text {
        "FM-1_015" => return "the official firmware V15".to_string(),
        "FM-1_016" => return "the official firmware V16".to_string(),
        _ => {}
    }
    if let Some((_, names)) = KNOWN.iter().find(|(product, _)| *product == text) {
        return match names.len() {
            1 => format!("{} ({text})", names[0]),
            2 => format!("{} or {} ({text})", names[0], names[1]),
            n => format!("one of {n} firmwares ({text})"),
        };
    }
    if let Some((_, _, name)) = FAMILIES.iter().find(|(prefix, digits, _)| {
        let Some(rest) = text.strip_prefix(*prefix) else {
            return false;
        };
        rest.len() == *digits && rest.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return format!("{name} ({text})");
    }
    format!("firmware {text}")
}

pub(crate) use fm1_base::package::{logical_image, product_of};
