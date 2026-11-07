    use super::*;

    #[test]
    fn loader_wait_can_stay_on_the_original_usb_path() {
        assert!(usb_matches(Some("1-2.4"), Some("1-2.4")));
        assert!(!usb_matches(Some("1-2.3"), Some("1-2.4")));
        // An unknown path on either side falls back to the model check.
        assert!(usb_matches(None, Some("1-2.4")));
        assert!(usb_matches(Some("1-2.4"), None));
    }

    #[test]
    fn model_matching_keeps_stock_units_on_their_own_build() {
        // The Donner reports its own name; M-VAVE documents the SMK-37 Pro build.
        assert!(same_model("STARRYKEY-37 PLAY", "SMK-37 Pro"));
        assert!(same_model("starrykey-37 play", "smk-37 pro"));
        assert!(same_model("SMK-37 Pro", "SMK-37 Pro"));
        assert!(!same_model("SMK-37 Elite", "SMK-37 Pro"));
        assert!(!same_model("MKE-P37", "SMK-37 Pro"));
        assert!(!same_model("FM-1", "SMK-37 Pro"));

        // A stock Starrykey (version 014) refuses an MKE-P37 package, as the unit did.
        assert!(package_fits("STARRYKEY-37 PLAY_014", "SMK-37 Pro_300"));
        assert!(!package_fits("STARRYKEY-37 PLAY_014", "MKE-P37_900"));
        assert!(!package_fits("STARRYKEY-37 PLAY_014", "SMK-37 Elite_900"));
        assert!(!package_fits("SMK-37 Pro_016", "MKE-P37_900"));
        // Custom firmware (7xx/9xx) accepts any SMK model, per the reference installers.
        assert!(package_fits("SMK-37 Pro_900", "MKE-P37_900"));
        assert!(package_fits("MKE-P37_900", "SMK-37 Pro_900"));
        assert_eq!(device_type("FM-1_916"), Some("FM-1"));
        assert_eq!(device_type("STARRYKEY-37 PLAY_014"), Some("SMK-37 Pro"));
        assert_eq!(device_type("SMK-37 Elite_016"), Some("SMK-37 Elite"));
        assert_eq!(device_type("MKE-P37_900"), Some("MKE-P37"));
        assert_eq!(device_type("ota-MKE-P37_900"), Some("MKE-P37"));
        assert_eq!(device_type("something else"), None);
        assert!(package_fits("ota-SMK-37 Pro_900", "SMK-37 Pro_0"));
        assert!(!package_fits("FM-1_916", "SMK-37 Pro_900"));
        assert!(!package_fits("SMK-37 Pro_900", "FM-1_916"));
    }

    #[test]
    fn post_flash_identity_accepts_the_model_family() {
        assert!(same_identity("SMK-37 Pro_300", "SMK-37 Pro_300"));
        assert!(same_identity("STARRYKEY-37 PLAY_300", "SMK-37 Pro_300"));
        assert!(!same_identity("STARRYKEY-37 PLAY_301", "SMK-37 Pro_300"));
        assert!(!same_identity("FM-1_300", "SMK-37 Pro_300"));
        assert!(!same_identity("SMK-37 Pro", "SMK-37 Pro_300"));
    }
