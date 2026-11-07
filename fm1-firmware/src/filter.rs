//! The Library's device selector: one family or SMK-37 submodel at a time.

use serde::{Deserialize, Deserializer, Serialize};

use crate::library::{Firmware, FirmwareDevice};

/// What the Library list and Download All include.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeviceFilter {
    /// Every firmware, FM-1 and all SMK-37 builds.
    All,
    /// FM-1 only (the emulator panes).
    #[default]
    Fm1,
    Smk37Pro,
    Smk37Elite,
    MkeP37,
}

impl DeviceFilter {
    pub const ALL: [DeviceFilter; 5] = [
        Self::All,
        Self::Fm1,
        Self::Smk37Pro,
        Self::Smk37Elite,
        Self::MkeP37,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Fm1 => "FM-1",
            Self::Smk37Pro => "SMK-37 Pro",
            Self::Smk37Elite => "SMK-37 Elite",
            Self::MkeP37 => "MKE-P37",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }

    pub fn shows(self, firmware: &Firmware) -> bool {
        match self {
            Self::All => true,
            Self::Fm1 => firmware.device == FirmwareDevice::Fm1,
            Self::Smk37Pro => firmware.variant() == Some("SMK-37 Pro"),
            Self::Smk37Elite => firmware.variant() == Some("SMK-37 Elite"),
            Self::MkeP37 => firmware.variant() == Some("MKE-P37"),
        }
    }
}

/// Old records stored two booleans; carry them into the single choice.
impl<'de> Deserialize<'de> for DeviceFilter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Name(String),
            Old {
                #[serde(default)]
                fm1: bool,
                #[serde(default)]
                smk37: bool,
            },
        }
        Ok(match Repr::deserialize(deserializer)? {
            Repr::Name(name) => match name.as_str() {
                "all" => Self::All,
                "fm1" => Self::Fm1,
                "smk37-pro" | "smk37pro" => Self::Smk37Pro,
                "smk37-elite" => Self::Smk37Elite,
                "mke-p37" => Self::MkeP37,
                _ => Self::All,
            },
            Repr::Old { fm1, smk37 } => match (fm1, smk37) {
                (true, false) => Self::Fm1,
                (true, true) => Self::All,
                // The old SMK-only view had no submodel; keep showing all SMK rows.
                (false, true) => Self::All,
                (false, false) => Self::All,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn firmware(device: FirmwareDevice, variant: Option<&str>) -> Firmware {
        Firmware {
            name: "x".into(),
            byline: "a".into(),
            version: "1".into(),
            description: String::new(),
            tags: variant.map(|v| vec![v.to_string()]).unwrap_or_default(),
            device,
            upgradable: false,
            status: crate::library::Status::Available,
            website: None,
            source_url: None,
            source: crate::library::FirmwareSource::Builtin,
        }
    }

    #[test]
    fn each_selection_shows_only_its_submodel() {
        let fm1 = firmware(FirmwareDevice::Fm1, None);
        let pro = firmware(FirmwareDevice::Smk37, Some("SMK-37 Pro"));
        let elite = firmware(FirmwareDevice::Smk37, Some("SMK-37 Elite"));
        let mke = firmware(FirmwareDevice::Smk37, Some("MKE-P37"));
        assert!(DeviceFilter::All.shows(&pro) && DeviceFilter::All.shows(&fm1));
        assert!(DeviceFilter::Fm1.shows(&fm1) && !DeviceFilter::Fm1.shows(&pro));
        assert!(DeviceFilter::Smk37Pro.shows(&pro) && !DeviceFilter::Smk37Pro.shows(&elite));
        assert!(DeviceFilter::Smk37Elite.shows(&elite) && !DeviceFilter::Smk37Elite.shows(&mke));
        assert!(DeviceFilter::MkeP37.shows(&mke) && !DeviceFilter::MkeP37.shows(&fm1));
    }

    #[test]
    fn old_boolean_records_migrate() {
        use serde_json::json;
        assert_eq!(
            serde_json::from_value::<DeviceFilter>(json!({"fm1": true, "smk37": false})).unwrap(),
            DeviceFilter::Fm1
        );
        assert_eq!(
            serde_json::from_value::<DeviceFilter>(json!({"fm1": true, "smk37": true})).unwrap(),
            DeviceFilter::All
        );
        assert_eq!(
            serde_json::from_value::<DeviceFilter>(json!({"fm1": false, "smk37": true})).unwrap(),
            DeviceFilter::All
        );
        assert_eq!(
            serde_json::from_value::<DeviceFilter>(json!("smk37-elite")).unwrap(),
            DeviceFilter::Smk37Elite
        );
    }
}
