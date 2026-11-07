    use super::*;
    use crate::builtin::{BUILTINS, BUNDLED_INDEX};

    fn temp_store(tag: &str) -> (Store, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "fm1-grid-library-test-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (Store::in_dir(&dir), dir)
    }

    fn hub_offer(name: &str, version: &str, file: &str) -> Firmware {
        Firmware {
            name: name.to_string(),
            byline: "hub author".to_string(),
            version: version.to_string(),
            description: "a hub firmware".to_string(),
            tags: vec!["kind".to_string(), "source:hub".to_string()],
            device: FirmwareDevice::Fm1,
            website: None,
            source_url: None,
            upgradable: false,
            status: Status::Available,
            source: FirmwareSource::Hub {
                path: format!("fw/{file}"),
                blob: None,
            },
        }
    }

    #[test]
    fn store_round_trip() {
        let (store, dir) = temp_store("roundtrip");
        let mut library = Library::load(&store);
        let file = dir.join("imported.fwsc");
        std::fs::write(&file, b"firmware bytes").unwrap();
        library.import_file(file.to_str().unwrap(), "imported", "1.2", "mine");
        let reloaded = Library::load(&store);
        let stored = &reloaded.stored;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].name, "imported");
        assert_eq!(stored[0].version, "1.2");
        assert!(matches!(stored[0].source, FirmwareSource::File { .. }));
        // The computed status must not have been persisted.
        let json = std::fs::read_to_string(dir.join("objects/library.json")).unwrap();
        assert!(!json.contains("status"));
        std::fs::remove_dir_all(dir).ok();
    }

    fn write_package(path: &std::path::Path, identity: &str) {
        let blocks = 20;
        let mut bytes = vec![0xAA; (blocks + 1) * 0x30 + 16];
        for i in 0..blocks {
            bytes[i * 0x30 + 0x2F] = if i < identity.len() {
                identity.as_bytes()[i].wrapping_add(i as u8).wrapping_add(1)
            } else {
                0x7D
            };
        }
        bytes[blocks * 0x30 + 0x2F] = 0xFF;
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn import_reads_the_model_from_the_package() {
        let (store, dir) = temp_store("import-model");
        let mut library = Library::load(&store);
        let file = dir.join("smk.fwsc");
        write_package(&file, "SMK-37 Pro_900");
        library.import_file(file.to_str().unwrap(), "Sloop Pro", "2.8.5", "smk");
        let entry = library.stored.iter().find(|f| f.name == "Sloop Pro").unwrap();
        assert_eq!(entry.device, FirmwareDevice::Smk37);
        assert_eq!(entry.variant(), Some("SMK-37 Pro"));
        assert!(crate::filter::DeviceFilter::Smk37Pro.shows(entry));

        // The same name for an FM-1 package is a different row.
        write_package(&file, "FM-1_916");
        library.import_file(file.to_str().unwrap(), "Sloop Pro", "2.8.5", "fm1");
        assert_eq!(
            library.stored.iter().filter(|f| f.name == "Sloop Pro").count(),
            2
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_file_is_missing() {
        let (store, dir) = temp_store("missing");
        let mut library = Library::load(&store);
        library.import_file("/no/such/file.fwsc", "ghost", "1.0", "gone");
        let entry = library.list().iter().find(|f| f.name == "ghost").unwrap().clone();
        assert_eq!(entry.status, Status::Missing);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn hub_never_shadows_builtin_and_flags_upgradable() {
        let (store, dir) = temp_store("shadow");
        let mut library = Library::load(&store);
        let builtin = BUILTINS.first().expect("no builtins");
        *library.hub.lock().unwrap() = vec![
            hub_offer(builtin.name, "999.0", "newer.fwsc"),
            hub_offer("hub-only", "1.0", "hubonly.fwsc"),
        ];
        let view = library.list();
        // The builtin appears once, under its own name, flagged upgradable.
        let matches: Vec<_> = view.iter().filter(|f| f.name == builtin.name).collect();
        assert_eq!(matches.len(), 1);
        assert!(matches!(matches[0].source, FirmwareSource::Builtin));
        assert!(matches[0].upgradable);
        // Same version is not an upgrade.
        let mut library = Library::load(&store);
        *library.hub.lock().unwrap() = vec![hub_offer(builtin.name, builtin.version, "same.fwsc")];
        let view = library.list();
        let matches: Vec<_> = view.iter().filter(|f| f.name == builtin.name).collect();
        assert_eq!(matches.len(), 1);
        assert!(!matches[0].upgradable);
        // A hub-only name is listed after the installed entries.
        let mut library = Library::load(&store);
        *library.hub.lock().unwrap() = vec![hub_offer("hub-only", "1.0", "hubonly.fwsc")];
        let view = library.list();
        let position = view.iter().position(|f| f.name == "hub-only").unwrap();
        assert_eq!(position, view.len() - 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn installed_hub_entry_is_upgradable() {
        let (store, dir) = temp_store("hub-upgrade");
        let mut library = Library::load(&store);
        // Installed at 1.0 (blob stored); the listing then moves to 2.0.
        let key = blob_key("fw/pkg.fwsc");
        store.store_blob(&key, b"package").unwrap();
        let mut installed = hub_offer("pkg", "1.0", "pkg.fwsc");
        installed.source = FirmwareSource::Hub {
            path: "fw/pkg.fwsc".to_string(),
            blob: Some(key),
        };
        library.stored.push(installed);
        *library.hub.lock().unwrap() = vec![hub_offer("pkg", "2.0", "pkg-2.fwsc")];
        library.view_dirty = true;

        let view = library.list();
        let entry = view.iter().find(|f| f.name == "pkg").unwrap();
        assert!(entry.upgradable, "an installed hub package flags upgradable");
        // The offer hides behind the install: one row, and the sweep offers it.
        assert_eq!(view.iter().filter(|f| f.name == "pkg").count(), 1);
        assert_eq!(library.next_download().as_deref(), Some("pkg"));

        // Caught up: the same version neither flags nor sweeps.
        *library.hub.lock().unwrap() = vec![hub_offer("pkg", "1.0", "pkg.fwsc")];
        library.view_dirty = true;
        let view = library.list();
        assert!(!view.iter().find(|f| f.name == "pkg").unwrap().upgradable);
        assert_eq!(library.next_download(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn download_all_follows_the_device_filter() {
        let (store, dir) = temp_store("device-filter");
        let mut library = Library::load(&store);
        let mut smk = hub_offer("smk-thing", "1.0", "smk.fwsc");
        smk.device = FirmwareDevice::Smk37;
        smk.tags.push("SMK-37 Pro".to_string());
        *library.hub.lock().unwrap() = vec![smk, hub_offer("fm1-thing", "1.0", "fm1.fwsc")];
        library.view_dirty = true;

        library.set_device_filter(Some(crate::filter::DeviceFilter::Fm1));
        assert_eq!(library.next_download().as_deref(), Some("fm1-thing"));
        library.set_device_filter(Some(crate::filter::DeviceFilter::All));
        assert_eq!(library.next_download().as_deref(), Some("smk-thing"));
        library.set_device_filter(Some(crate::filter::DeviceFilter::Smk37Pro));
        assert_eq!(library.next_download().as_deref(), Some("smk-thing"));
        // None means no UI filter: everything is offered.
        library.set_device_filter(None);
        assert_eq!(library.next_download().as_deref(), Some("smk-thing"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn next_download_walks_and_finishes() {
        let (store, dir) = temp_store("next");
        let builtin = BUILTINS.first().expect("no builtins");
        let mut library = Library::load(&store);
        // The load seeds the binary's index-only offers; this test owns the listing.
        library.hub.lock().unwrap().clear();

        // No hub listing: nothing to fetch.
        assert_eq!(library.next_download(), None);

        // An upgradable builtin comes before a hub-only offer (list order: builtins
        // first); the upgrade downloads under the builtin's name.
        *library.hub.lock().unwrap() = vec![
            hub_offer(builtin.name, "999.0", "newer.fwsc"),
            hub_offer("hub-only", "1.0", "hubonly.fwsc"),
        ];
        library.view_dirty = true;
        assert_eq!(library.next_download().as_deref(), Some(builtin.name));

        // One at a time: an in-flight download parks the pump.
        library.downloads.insert(
            "hub-only".to_string(),
            Arc::new(Download {
                percent: AtomicU32::new(0),
                outcome: Mutex::new(None),
            }),
        );
        assert_eq!(library.next_download(), None);
        library.downloads.clear();

        // A failed name is skipped, not retried forever.
        library.download_failed.insert(builtin.name.to_string());
        assert_eq!(library.next_download().as_deref(), Some("hub-only"));

        // Once the hub's newer package sits downloaded under the builtin's name,
        // the upgrade is satisfied — only the offer remains.
        let mut library = Library::load(&store);
        *library.hub.lock().unwrap() = vec![
            hub_offer(builtin.name, "999.0", "newer.fwsc"),
            hub_offer("hub-only", "1.0", "hubonly.fwsc"),
        ];
        let key = blob_key("fw/newer.fwsc");
        store.store_blob(&key, b"package").unwrap();
        let mut done = hub_offer(builtin.name, "999.0", "newer.fwsc");
        done.source = FirmwareSource::Hub {
            path: "fw/newer.fwsc".to_string(),
            blob: Some(key),
        };
        library.stored.push(done);
        library.view_dirty = true;
        assert_eq!(library.next_download().as_deref(), Some("hub-only"));

        // Everything at hand: nothing left.
        library.download_failed.insert("hub-only".to_string());
        assert_eq!(library.next_download(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    /// Download All is event-driven: a sweep runs on toggle-on, on a hub swap, and
    /// when a download finishes — never on a poll. A failed item is marked and the
    /// walk moves past it; with nothing left the pump stays parked.
    #[test]
    fn download_all_sweeps_on_events() {
        let (store, dir) = temp_store("all");
        let mut library = Library::load(&store);
        // The load seeds the binary's index-only offers; this test owns the listing.
        library.hub.lock().unwrap().clear();
        let offer = |name: &str| Firmware {
            source: FirmwareSource::Hub {
                // A closed port fails fast and deterministically, offline.
                path: format!("http://127.0.0.1:1/fw/{name}.fwsc"),
                blob: None,
            },
            ..hub_offer(name, "1.0", "ignored.fwsc")
        };

        // Toggle-on with an empty listing sweeps without finding anything, and a
        // pump with nothing moving is a no-op.
        library.set_download_all(true);
        assert!(library.downloads.is_empty());
        library.pump_downloads();
        assert!(library.downloads.is_empty());

        // A hub swap is a trigger: two closed-port offers, both doomed to fail fast.
        *library.hub.lock().unwrap() = vec![offer("fail-one"), offer("fail-two")];
        library.hub_swapped.store(true, Ordering::Relaxed);
        library.pump_downloads();
        assert!(
            library.downloads.contains_key("fail-one"),
            "the swap should kick the first download"
        );

        // A finished download kicks the next step: fail-one errors out, the pump
        // marks it and the walk moves down the list to fail-two.
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            library.pump_downloads();
            if library.downloads.contains_key("fail-two") {
                break;
            }
        }
        assert!(library.download_failed.contains("fail-one"));
        assert!(
            library.downloads.contains_key("fail-two"),
            "the walk should move past the failure"
        );

        // fail-two fails too; the listing is exhausted and the pump parks.
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            library.pump_downloads();
            if library.downloads.is_empty() {
                break;
            }
        }
        assert!(library.download_failed.contains("fail-two"));
        assert!(library.downloads.is_empty());
        library.pump_downloads();
        assert!(library.downloads.is_empty(), "nothing left: no retry loop");

        // A fresh toggle-on clears the marks and retries once.
        library.set_download_all(false);
        library.set_download_all(true);
        assert!(library.downloads.contains_key("fail-one"));
        library.set_download_all(false);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn version_compare_is_piecewise_numeric() {
        assert!(version_newer("1.10", "1.9"));
        assert!(!version_newer("1.9", "1.10"));
        assert!(!version_newer("1.10", "1.10"));
        assert!(version_newer("2.0", "1.10"));
        assert!(version_newer("1.2.1", "1.2"));
        assert!(!version_newer("1.2", "1.2.1"));
    }

    #[test]
    fn remove_rules() {
        let (store, dir) = temp_store("remove");
        let mut library = Library::load(&store);
        // Builtins can't be removed.
        let builtin_name = BUILTINS.first().unwrap().name.to_string();
        library.remove(&builtin_name);
        assert!(library.list().iter().any(|f| f.name == builtin_name));
        // A File entry is dropped; the file itself stays.
        let file = dir.join("keep.fwsc");
        std::fs::write(&file, b"bytes").unwrap();
        library.import_file(file.to_str().unwrap(), "imported", "1.0", "");
        library.remove("imported");
        assert!(!library.list().iter().any(|f| f.name == "imported"));
        assert!(file.is_file());
        assert!(Library::load(&store).stored.is_empty());
        // A Hub entry is dropped along with its blob.
        let mut entry = hub_offer("downloaded", "1.0", "dl.fwsc");
        if let FirmwareSource::Hub { path, blob } = &mut entry.source {
            let key = blob_key(path);
            store.store_blob(&key, b"package").unwrap();
            *blob = Some(key);
        }
        library.stored.push(entry);
        library.persist();
        library.remove("downloaded");
        assert!(!library.list().iter().any(|f| f.name == "downloaded"));
        assert!(store.list_blobs(None).unwrap().is_empty());
        assert!(Library::load(&store).stored.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn html_downloads_fail_without_storing_a_blob() {
        let (store, dir) = temp_store("html");
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            // Two requests: the .fwsc URL's 404-style page, then the install page.
            for _ in 0..2 {
                let Ok((mut socket, _)) = listener.accept() else { break };
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf);
                let body = b"<!DOCTYPE html><html><body>page</body></html>";
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(head.as_bytes()).ok();
                socket.write_all(body).ok();
            }
        });
        let mut library = Library::load(&store);
        let offer = |name: &str, url: String| Firmware {
            source: FirmwareSource::Hub {
                path: url,
                blob: None,
            },
            ..hub_offer(name, "1.0", "ignored.fwsc")
        };
        let base = format!("http://127.0.0.1:{port}");
        // A package-looking URL that serves HTML, and an install page naming no
        // package: both fail loudly instead of storing a page that reads Missing
        // forever.
        *library.hub.lock().unwrap() = vec![
            offer("error-page", format!("{base}/dl/x.fwsc")),
            offer("install-page", format!("{base}/install/")),
        ];
        for name in ["error-page", "install-page"] {
            library.start_download(name);
            for _ in 0..100 {
                std::thread::sleep(Duration::from_millis(50));
                if !library.downloads.contains_key(name) {
                    break;
                }
                library.collect_downloads();
            }
            library.collect_downloads();
            assert!(
                library.download_failed.contains(name),
                "{name} must be marked failed"
            );
        }
        assert!(store.list_blobs(None).unwrap().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn merge_combines_tags_and_prefers_the_newer_side() {
        let hub = vec![
            hub_offer("zp12", "2.5", "zp12-2.5.fwsc"),
            hub_offer("Rainbow Mode", "1.1.5-rb", "rainbow.fwsc"),
        ];
        // The editor has a newer zp12: its version and URL win, its tags join in,
        // and its origin tag replaces the hub's.
        let mut newer = hub_offer("zp12", "3.0", "ignored.fwsc");
        newer.tags = vec!["kind".to_string(), "drums".to_string(), "source:fm1-editor".to_string()];
        newer.byline = "editor author".to_string();
        newer.website = Some("https://zp12.example.com".to_string());
        newer.source_url = Some("https://github.com/x/zp12".to_string());
        newer.source = FirmwareSource::Hub {
            path: "https://example.com/zp12-3.0.fwsc".to_string(),
            blob: None,
        };
        // Same Rainbow Mode, spelled differently and older: tags merge, the hub's
        // version, URL and origin tag stay; the offer's links fill the gaps.
        let mut older = hub_offer("rainbow mode", "1.0", "ignored.fwsc");
        older.tags = vec!["ambient".to_string(), "source:fm1-editor".to_string()];
        older.website = Some("https://rainbow.example.com".to_string());
        older.source = FirmwareSource::Hub {
            path: "https://example.com/rainbow.fwsc".to_string(),
            blob: None,
        };
        // An editor-only entry appends.
        let only = hub_offer("editor-only", "0.1", "only.fwsc");
        let merged = merge_listings(hub, vec![newer, older, only]);
        assert_eq!(merged.len(), 3);
        let zp12 = merged.iter().find(|f| f.name == "zp12").unwrap();
        assert_eq!(zp12.version, "3.0");
        assert_eq!(zp12.tags, vec!["kind", "source:fm1-editor", "drums"]);
        assert_eq!(zp12.website.as_deref(), Some("https://zp12.example.com"));
        assert_eq!(zp12.source_url.as_deref(), Some("https://github.com/x/zp12"));
        // The hub stays the source of truth for the rest.
        assert_eq!(zp12.byline, "hub author");
        match &zp12.source {
            FirmwareSource::Hub { path, .. } => {
                assert_eq!(path, "https://example.com/zp12-3.0.fwsc")
            }
            _ => panic!("hub source expected"),
        }
        let rainbow = merged.iter().find(|f| f.name == "Rainbow Mode").unwrap();
        assert_eq!(rainbow.version, "1.1.5-rb");
        assert_eq!(rainbow.tags, vec!["kind", "source:hub", "ambient"]);
        assert_eq!(rainbow.website.as_deref(), Some("https://rainbow.example.com"));
        assert_eq!(rainbow.source_url, None);
        match &rainbow.source {
            FirmwareSource::Hub { path, .. } => assert_eq!(path, "fw/rainbow.fwsc"),
            _ => panic!("hub source expected"),
        }
        assert_eq!(merged[2].name, "editor-only");
    }

    #[test]
    fn merge_deduplicates_name_spelling_variants() {
        // The live triplicate: hub "PurpleMonkey", fm1-editor "PurpleMonkey FM-1",
        // Reverbose "Purple Monkey", all by Quixotic7. Editor first, then
        // Reverbose — the refresh's fold order.
        let hub = vec![hub_offer("PurpleMonkey", "0.11", "purplemonkey.fwsc")];
        let mut editor = hub_offer("PurpleMonkey FM-1", "0.11", "ignored.fwsc");
        editor.tags = vec!["games".to_string(), "source:fm1-editor".to_string()];
        let mut reverbose = hub_offer("Purple Monkey", "", "ignored.fwsc");
        reverbose.tags = vec!["source:reverbose".to_string()];
        let merged = merge_listings(merge_listings(hub, vec![editor]), vec![reverbose]);
        assert_eq!(merged.len(), 1);
        let entry = &merged[0];
        // The hub's name, version, download and origin tag survive; the equal
        // version and the empty one never supersede.
        assert_eq!(entry.name, "PurpleMonkey");
        assert_eq!(entry.version, "0.11");
        assert_eq!(entry.tags, vec!["kind", "source:hub", "games"]);
        match &entry.source {
            FirmwareSource::Hub { path, .. } => assert_eq!(path, "fw/purplemonkey.fwsc"),
            _ => panic!("hub source expected"),
        }
    }

    #[test]
    fn merge_folds_substring_name_variants_by_the_same_author() {
        let hub = vec![
            hub_offer("OPX", "0.2.0", "opx.fwsc"),
            hub_offer("X0X", "1.1.0", "x0x.fwsc"),
            hub_offer("Tuna", "1.0", "tuna.fwsc"),
        ];
        // "OPX Solo" and "X0X-RMX" grow the name by a word or suffix: by the same
        // author they are spelling variants now.
        let solo = hub_offer("OPX Solo", "0.2.0", "ignored.fwsc");
        let rmx = hub_offer("X0X-RMX", "1.1.0", "ignored.fwsc");
        // A device-suffix spelling from a different author is someone else's
        // firmware, not a variant.
        let mut other = hub_offer("Tuna FM-1", "1.0", "ignored.fwsc");
        other.byline = "another author".to_string();
        let merged = merge_listings(hub, vec![solo, rmx, other]);
        assert_eq!(merged.len(), 4);
        assert!(merged.iter().any(|f| f.name == "OPX"));
        assert!(merged.iter().any(|f| f.name == "X0X"));
        assert!(merged.iter().any(|f| f.name == "Tuna"));
        assert!(merged.iter().any(|f| f.name == "Tuna FM-1"));
        assert!(!merged.iter().any(|f| f.name == "OPX Solo"));
        assert!(!merged.iter().any(|f| f.name == "X0X-RMX"));
    }

    #[test]
    fn model_variants_are_never_the_same_firmware() {
        let mut pro = hub_offer("SLOOP Pro", "2.8.5", "pro.fwsc");
        pro.tags = vec!["groove".into(), "SMK-37 Pro".into(), "source:mutant".into()];
        let mut mke = hub_offer("SLOOP Pro (mke)", "2.8.5", "mke.fwsc");
        mke.tags = vec!["groove".into(), "MKE-P37".into(), "source:mutant".into()];
        let mut elite = hub_offer("SLOOP Pro (elite)", "2.8.5", "elite.fwsc");
        elite.tags = vec!["groove".into(), "SMK-37 Elite".into(), "source:mutant".into()];
        assert_eq!(pro.variant(), Some("SMK-37 Pro"));
        assert_eq!(mke.variant(), Some("MKE-P37"));
        assert_eq!(elite.variant(), Some("SMK-37 Elite"));

        assert!(!same_firmware(&pro, &mke));
        assert!(!same_firmware(&pro, &elite));
        assert!(!same_firmware(&mke, &elite));
        // The install row from one source still keeps the same model together.
        let mut pro_other = hub_offer("Sloop Pro", "2.8.5", "other.fwsc");
        pro_other.tags = vec!["SMK-37 Pro".into(), "source:reverbose".into()];
        assert!(same_firmware(&pro, &pro_other));

        let merged = merge_listings(vec![pro], vec![mke, elite, pro_other]);
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn same_firmware_matches_substrings_only_for_the_same_author() {
        let hub = hub_offer("OPX", "0.2.0", "opx.fwsc");
        let mut extended = hub_offer("OPX Solo", "0.2.0", "ignored.fwsc");
        assert!(same_firmware(&hub, &extended));
        // Authors compare case-insensitively, Unicode included.
        extended.byline = "HUB AUTHOR".to_string();
        assert!(same_firmware(&hub, &extended));
        let mut unicode = hub_offer("Felucca Salt", "0.9", "ignored.fwsc");
        unicode.byline = "HÜGELTON INSTRUMENTS".to_string();
        let mut base = hub_offer("Felucca", "1.8", "felucca.fwsc");
        base.byline = "Hügelton Instruments".to_string();
        assert!(same_firmware(&base, &unicode));
        // Either direction, and a different author keeps them apart.
        let mut long = hub_offer("SLOOP Floyd FM", "2.4", "ignored.fwsc");
        long.byline = "ISOD89".to_string();
        let mut short = hub_offer("SLOOP", "2.8.5", "sloop.fwsc");
        short.byline = "isod89".to_string();
        assert!(same_firmware(&long, &short));
        extended.byline = "someone else".to_string();
        assert!(!same_firmware(&hub, &extended));
    }

    #[test]
    fn same_name_on_two_devices_stays_two_rows() {
        // OberVave ships an FM-1 and an SMK-37 package under one name; merging them
        // would lose the SMK-37 build (and a newer one could hand the FM-1 row the
        // SMK package).
        let hub = vec![hub_offer("OberVave", "3.3.2", "obervave.fwsc")];
        let mut smk = hub_offer("OberVave", "9.0", "obervave-9-smk37.fwsc");
        smk.device = FirmwareDevice::Smk37;
        smk.tags = vec!["synth".to_string(), "source:mutant".to_string()];
        let merged = merge_listings(hub, vec![smk]);
        assert_eq!(merged.len(), 2);
        let fm1 = merged
            .iter()
            .find(|f| f.device == FirmwareDevice::Fm1)
            .unwrap();
        assert_eq!(fm1.version, "3.3.2");
        match &fm1.source {
            FirmwareSource::Hub { path, .. } => assert_eq!(path, "fw/obervave.fwsc"),
            _ => panic!("hub source expected"),
        }
        let smk = merged
            .iter()
            .find(|f| f.device == FirmwareDevice::Smk37)
            .unwrap();
        assert_eq!(smk.version, "9.0");
    }

    #[test]
    fn persist_merges_entries_added_by_another_process() {
        let (store, dir) = temp_store("persist-merge");
        let mut gui = Library::load(&store);
        let mut tui = Library::load(&store);

        gui.stored.push(hub_offer("Gui Only", "1.0", "gui.fwsc"));
        gui.persist();
        tui.stored.push(hub_offer("Tui Only", "1.0", "tui.fwsc"));
        tui.persist();

        let reloaded = Library::load(&store);
        assert!(reloaded.stored.iter().any(|f| f.name == "Gui Only"));
        assert!(reloaded.stored.iter().any(|f| f.name == "Tui Only"));
        // The writer picked up the other app's entry into its own view too.
        assert!(tui.stored.iter().any(|f| f.name == "Gui Only"));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn installed_variant_spelling_shadows_the_offer() {
        let (store, dir) = temp_store("shadow");
        let mut library = Library::load(&store);
        // Downloaded when the editor listing called it "PurpleMonkey FM-1"; the
        // stored entry keeps the byline it was scraped with.
        let installed = hub_offer("PurpleMonkey FM-1", "0.11", "ignored.fwsc");
        library.stored.push(installed);
        // The merged listing now carries the hub's spelling.
        *library.hub.lock().unwrap() = vec![hub_offer("PurpleMonkey", "0.11", "pm.fwsc")];
        let names: Vec<&str> = library
            .list()
            .iter()
            .map(|f| f.name.as_str())
            .filter(|name| name.to_lowercase().contains("purple"))
            .collect();
        assert_eq!(names, vec!["PurpleMonkey FM-1"]);
        std::fs::remove_dir_all(dir).ok();
    }

    /// Entries stored before origin tags existed get one from their source at load;
    /// an absolute Hub path means a pre-tag fm1-editor download (the hub's own
    /// entries carry relative paths).
    #[test]
    fn legacy_entries_gain_origin_tags() {
        let (store, dir) = temp_store("origin-tags");
        let mut hub_row = hub_offer("Old Hub Row", "1.0", "x.fwsc");
        hub_row.tags.retain(|tag| !tag.starts_with("source:"));
        hub_row.source = FirmwareSource::Hub {
            path: "fw/x.fwsc".to_string(),
            blob: Some("hub/x.fwsc".to_string()),
        };
        let mut editor_row = hub_offer("Old Editor Row", "1.0", "y.fwsc");
        editor_row.tags.retain(|tag| !tag.starts_with("source:"));
        editor_row.source = FirmwareSource::Hub {
            path: "https://fm1-editor.com/firmware/y".to_string(),
            blob: Some("ext/y".to_string()),
        };
        store
            .store_object(LIBRARY_OBJECT, &vec![hub_row, editor_row])
            .unwrap();
        let library = Library::load(&store);
        let tag_of = |name: &str| {
            library
                .stored
                .iter()
                .find(|f| f.name == name)
                .unwrap()
                .tags
                .iter()
                .find(|tag| tag.starts_with("source:"))
                .cloned()
        };
        assert_eq!(tag_of("Old Hub Row").as_deref(), Some("source:hub"));
        assert_eq!(tag_of("Old Editor Row").as_deref(), Some("source:fm1-editor"));
        std::fs::remove_dir_all(dir).ok();
    }

    /// The variant-spelled row must still download: the version bump flags it
    /// upgradable, Download All targets it by its own name, `start_download`
    /// resolves the hub's offer (an exact-name lookup would miss), and completion
    /// replaces the variant row with the hub-spelled one.
    #[test]
    fn variant_spelling_upgrades_through_the_offer() {
        let (store, dir) = temp_store("variant-upgrade");
        let mut library = Library::load(&store);
        library.stored.push(hub_offer("PurpleMonkey FM-1", "0.11", "ignored.fwsc"));
        // A dead URL: start_download's fetch thread fails fast and offline (its
        // error lands in an Arc this test replaces before collecting).
        let mut offer = hub_offer("PurpleMonkey", "0.12", "pm.fwsc");
        offer.source = FirmwareSource::Hub {
            path: "http://127.0.0.1:1/pm.fwsc".to_string(),
            blob: None,
        };
        *library.hub.lock().unwrap() = vec![offer];

        let row = library
            .list()
            .iter()
            .find(|f| f.name == "PurpleMonkey FM-1")
            .unwrap()
            .clone();
        assert!(row.upgradable, "hub's 0.12 moves ahead of the stored 0.11");
        assert_eq!(library.next_download().as_deref(), Some("PurpleMonkey FM-1"));

        // The row's own name arms the download; the offer resolves fuzzily.
        library.start_download("PurpleMonkey FM-1");
        assert!(library.downloads.contains_key("PurpleMonkey FM-1"));
        assert_eq!(entry_status(&row, &library.downloads, &store), Status::Downloading);

        // Completion folds the package in under the hub's name, replacing the
        // variant row (the blob is stored for real so the status stats Available).
        let path = "http://127.0.0.1:1/pm.fwsc";
        store.store_blob(&blob_key(path), b"\x00\x01 firmware").unwrap();
        library.downloads.insert(
            "PurpleMonkey FM-1".to_string(),
            Arc::new(Download {
                percent: AtomicU32::new(100),
                outcome: Mutex::new(Some(Ok(Firmware {
                    status: Status::Available,
                    source: FirmwareSource::Hub {
                        path: path.to_string(),
                        blob: Some(blob_key(path)),
                    },
                    ..hub_offer("PurpleMonkey", "0.12", "pm.fwsc")
                }))),
            }),
        );
        assert!(library.collect_downloads());
        let names: Vec<&str> = library
            .list()
            .iter()
            .map(|f| f.name.as_str())
            .filter(|name| name.to_lowercase().contains("purple"))
            .collect();
        assert_eq!(names, vec!["PurpleMonkey"], "the variant row is replaced");
        // Versions equal: nothing re-arms, Download All parks.
        assert_eq!(library.next_download(), None);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn absolute_urls_key_uniquely_and_download_as_given() {
        // Hub-relative paths behave exactly as before.
        assert_eq!(blob_key("fw/x.fwsc"), "hub/x.fwsc");
        assert_eq!(
            hub_download_url("fw/x.fwsc"),
            "https://thefirmwarehub.com/fw/x.fwsc"
        );
        // Absolute URLs pass through to the fetch and key on the whole URL: two
        // "latest" release pages must not share a blob.
        let a = "https://github.com/a/proj/releases/latest";
        let b = "https://github.com/b/proj/releases/latest";
        assert_eq!(hub_download_url(a), a);
        assert!(blob_key(a).starts_with("ext/latest-"));
        assert_ne!(blob_key(a), blob_key(b));
        // Queries stay out of the file name.
        assert!(blob_key("https://x.test/fw/p.fwsc?raw=1").starts_with("ext/p.fwsc-"));
    }

    #[test]
    fn hub_refresh_keeps_an_hourly_cadence() {
        let (store, dir) = temp_store("cadence");
        let mut library = Library::load(&store);
        library.refresh_hub_from(
            "http://127.0.0.1:1/catalog.json".to_string(),
            "http://127.0.0.1:1/firmware/".to_string(),
            "http://127.0.0.1:1/mutant.json".to_string(),
            "http://127.0.0.1:1/h/m-vave/fm-1".to_string(),
            "http://127.0.0.1:1/h/m-vave/smk-37".to_string(),
        );
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(50));
            if matches!(library.hub_status(), HubStatus::Failed(_)) {
                break;
            }
        }
        assert!(matches!(library.hub_status(), HubStatus::Failed(_)));
        // Within the hour, an automatic refresh is a no-op — the status must not
        // flick back to Loading (and nothing goes on the network).
        library.refresh_hub();
        assert!(matches!(library.hub_status(), HubStatus::Failed(_)));
        // The button always goes.
        library.reload_hub();
        assert!(matches!(library.hub_status(), HubStatus::Loading));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn hub_status_reports_loading_then_failure() {
        let (store, dir) = temp_store("hubstatus");
        let mut library = Library::load(&store);
        // A closed port fails fast and deterministically.
        library.refresh_hub_from(
            "http://127.0.0.1:1/catalog.json".to_string(),
            "http://127.0.0.1:1/firmware/".to_string(),
            "http://127.0.0.1:1/mutant.json".to_string(),
            "http://127.0.0.1:1/h/m-vave/fm-1".to_string(),
            "http://127.0.0.1:1/h/m-vave/smk-37".to_string(),
        );
        assert!(matches!(library.hub_status(), HubStatus::Loading));
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if matches!(library.hub_status(), HubStatus::Failed(_)) {
                break;
            }
        }
        assert!(matches!(library.hub_status(), HubStatus::Failed(_)));
        std::fs::remove_dir_all(dir).ok();
    }

    /// Listing-only entries ship metadata and a URL, never bytes; the baked table
    /// leaves them out and the hub carries them as downloadable offers from load.
    #[test]
    fn index_only_entries_are_offers_not_builtins() {
        let (store, dir) = temp_store("indexonly");
        let library = Library::load(&store);
        let hub = library.hub.lock().unwrap();
        assert_eq!(
            hub.len(),
            BUNDLED_INDEX.len(),
            "the seeded hub is exactly the index-only listing"
        );
        for b in BUNDLED_INDEX {
            assert!(b.bytes.is_empty(), "{} must not be embedded", b.name);
            assert!(!BUILTINS.iter().any(|baked| baked.name == b.name));
            let offer = hub
                .iter()
                .find(|firmware| firmware.name == b.name)
                .expect("index-only entry in the seeded hub");
            assert!(matches!(
                offer.source,
                FirmwareSource::Hub { ref path, blob: None } if path == b.url
            ));
            assert!(offer.tags.iter().any(|tag| tag == "source:index"));
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_751), (2026, 10, 25));
        // Round-trip sanity across a span including leap years.
        for days in [-719_162, -1, 0, 1, 18_282, 20_768, 29_303] {
            let (y, m, d) = civil_from_days(days);
            assert!((1..=12).contains(&m) && (1..=31).contains(&d) && y >= 1);
        }
    }
