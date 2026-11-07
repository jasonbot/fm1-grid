use super::*;

/// The whole update on its own thread, mirroring the firmware hub's one-shot flow:
/// step 1 on the running firmware, wait for the update loader the unit reboots into,
/// step 2 on the loader, wait for the unit to come back, verify the new identity. The
/// phases go to the device's row and to the pane-level board (rows come and go as the
/// unit reboots); the outcome lands on the row of whatever device is left at the end.
#[allow(clippy::too_many_arguments)]
pub(crate) fn flash_task(
    devices: Arc<Mutex<Vec<Device>>>,
    board: Board,
    link: Arc<DeviceLink>,
    identity: Arc<Mutex<Option<String>>>,
    flash: Arc<Mutex<Option<FlashState>>>,
    outcome: Arc<Mutex<Option<String>>>,
    clock: Arc<Clock>,
    hw_id: usize,
    bytes: Vec<u8>,
) {
    let result = flash_flow(&devices, &board, &link, &identity, &clock, hw_id, bytes, &flash);
    if let Ok(mut state) = flash.lock() {
        *state = None;
    }
    // The device is never clocked during an update; put the routing back afterwards,
    // but only while the original device is still the one registered under this link.
    let registered = devices
        .lock()
        .ok()
        .is_some_and(|list| list.iter().any(|device| Arc::ptr_eq(&device.link, &link)));
    if registered {
        let target: Option<Arc<dyn MidiSink>> = Some(link.clone() as Arc<dyn MidiSink>);
        clock.set_hw_target(hw_id, target);
    }
    if let Ok(mut outcome) = outcome.lock() {
        *outcome = Some(match &result {
            Ok(product) => format!("flashed: {}", friendly(product)),
            Err(message) => message.clone(),
        });
    }
    settle_board(&board, &result);
}

/// The board after a flash, normal or rescue: success clears it (the returned device's
/// row carries the outcome); failures stay on it, since the row they belong to may be
/// gone — a rescue target never had one.
pub(crate) fn settle_board(board: &Board, result: &Result<String, String>) {
    if let Ok(mut state) = board.lock() {
        *state = match result {
            Ok(_) => None,
            Err(message) => {
                // The failure line joins the log, not replaces it.
                let mut log = state.as_ref().map(|s| s.log.clone()).unwrap_or_default();
                log.push(message.clone());
                Some(FlashState {
                    step: message.clone(),
                    progress: None,
                    error: true,
                    log,
                })
            }
        };
    }
}

/// Publish a phase to the row's flash state (if that row still exists) and the board.
pub(crate) fn publish(
    flash: &Mutex<Option<FlashState>>,
    board: &Board,
    text: &str,
    progress: Option<f32>,
) {
    if let Ok(mut state) = flash.lock() {
        if let Some(state) = state.as_mut() {
            state.step = text.to_string();
            state.progress = progress;
        }
    }
    if let Ok(mut state) = board.lock() {
        // The board keeps every phase line; the dialog's terminal shows the tail.
        let mut log = state.as_ref().map(|s| s.log.clone()).unwrap_or_default();
        log.push(text.to_string());
        if log.len() > 200 {
            log.drain(..log.len() - 200);
        }
        *state = Some(FlashState {
            step: text.to_string(),
            progress,
            error: false,
            log,
        });
    }
}

/// Publish just the progress (the phase text stays as it is).
pub(crate) fn publish_progress(
    flash: &Mutex<Option<FlashState>>,
    board: &Board,
    progress: f32,
) {
    if let Ok(mut state) = flash.lock() {
        if let Some(state) = state.as_mut() {
            state.progress = Some(progress);
        }
    }
    if let Ok(mut state) = board.lock() {
        if let Some(state) = state.as_mut() {
            state.progress = Some(progress);
        }
    }
}

/// What a successful device wait hands back: the link, its identity slot, its flash
/// state slot and the outcome slot (the two Strings are identity and outcome).
pub(crate) type FoundDevice = Option<(
    Arc<DeviceLink>,
    Arc<Mutex<Option<String>>>,
    Arc<Mutex<Option<FlashState>>>,
    Arc<Mutex<Option<String>>>,
)>;

/// Wait for a device the scanner (re-)registered whose identity matches `pred`, up to
/// `timeout`. Returns its handles, claimed for flashing (row state set, clock unplugged).
#[allow(clippy::too_many_arguments)]
pub(crate) fn wait_for_device(
    devices: &Mutex<Vec<Device>>,
    clock: &Clock,
    board: &Board,
    timeout: Duration,
    pred: impl Fn(&Device) -> bool,
) -> FoundDevice {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(list) = devices.lock() {
            for device in list.iter() {
                if !pred(device) {
                    continue;
                }
                // Claim it: the row shows the flash state and no clock bytes go in.
                if let Ok(mut state) = device.flash.lock() {
                    *state = Some(FlashState {
                        step: "flashing… do not unplug".to_string(),
                        progress: None,
                        error: false,
            log: Vec::new(),
                    });
                }
                clock.set_hw_target(device.hw_id, None);
                if let Ok(mut state) = board.lock() {
                    if let Some(state) = state.as_mut() {
                        state.progress = None;
                    }
                }
                return Some((
                    device.link.clone(),
                    device.identity.clone(),
                    device.flash.clone(),
                    device.outcome.clone(),
                ));
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// Whether a device sits on the USB path the flash started from. An unknown path
/// falls back to matching any unit, as before.
pub(crate) fn usb_matches(have: Option<&str>, want: Option<&str>) -> bool {
    match want {
        None => true,
        Some(want) => have.is_none_or(|have| have == want),
    }
}

pub(crate) fn on_usb(device: &Device, usb: Option<&str>) -> bool {
    usb_matches(device.usb.as_deref(), usb)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn flash_flow(
    devices: &Mutex<Vec<Device>>,
    board: &Board,
    link: &DeviceLink,
    identity: &Mutex<Option<String>>,
    clock: &Clock,
    hw_id: usize,
    bytes: Vec<u8>,
    flash: &Mutex<Option<FlashState>>,
) -> Result<String, String> {
    // Clock bytes would confuse both the running firmware and the loader.
    clock.set_hw_target(hw_id, None);

    let Some(product) = product_of(&bytes) else {
        return Err("not an FM-1 package: no product identity in the file".to_string());
    };
    let usb = devices.lock().ok().and_then(|list| {
        list.iter()
            .find(|device| device.hw_id == hw_id)
            .and_then(|device| device.usb.clone())
    });
    let model = product.split_once('_').map(|(model, _)| model).unwrap_or("FM-1");
    let image = logical_image(&bytes);
    let current = identity.lock().ok().and_then(|i| i.clone());
    let is_ota = current
        .as_deref()
        .is_some_and(|text| text.to_lowercase().starts_with("ota-"));
    let current_model = current
        .as_deref()
        .and_then(|text| text.split_once('_'))
        .map(|(model, _)| model.to_string());
    fn strip_ota(text: &str) -> &str {
        text.strip_prefix("ota-")
            .or_else(|| text.strip_prefix("OTA-"))
            .unwrap_or(text)
    }

    if !is_ota {
        // Step 1: the running firmware verifies the package and stages the loader.
        publish(flash, board, "checking the device's firmware…", None);
        let Some(current) = current else {
            return Err(
                "the device did not answer the handshake, so its firmware cannot be \
                 checked: nothing was sent"
                    .to_string(),
            );
        };
        if !package_fits(&current, &product) {
            return Err(format!(
                "the device is {current}, the package is for {product}: nothing was sent"
            ));
        }
        publish(flash, board, "sending the UPGRADE command…", None);
        link.send(&UPGRADE);
        thread::sleep(Duration::from_secs(2));
        publish(flash, board, "staging the update loader…", None);
        let served = serve(link, &image, &[FINISH_STAGE], Duration::from_secs(8), flash, board)?;
        if served.finish != Some(FINISH_STAGE) {
            return Err(format!(
                "the device stopped after {} requests: nothing was written",
                served.count
            ));
        }
        // The device reboots into its update loader and comes back as a new port; the
        // scanner registers it and this thread picks it up (the hub waits 30 s).
        publish(flash, board, "step 1 done: waiting for the update loader…", None);
        if let Ok(mut identity) = identity.lock() {
            *identity = None;
        }
        let loader = wait_for_device(devices, clock, board, Duration::from_secs(30), |device| {
            if !on_usb(device, usb.as_deref()) {
                return false;
            }
            let text = device.identity.lock().ok().and_then(|i| i.clone());
            text.is_some_and(|text| {
                text.to_lowercase().starts_with("ota-")
                    && strip_ota(&text)
                        .split_once('_')
                        .is_some_and(|(m, _)| same_model(m, model))
            })
        });
        let Some((loader_link, _, loader_flash, _)) = loader else {
            return Err(
                "the update loader did not appear within 30 s: replug the USB cable and \
                 press Flash again — the device stays in update mode until the update is \
                 finished"
                    .to_string(),
            );
        };
        let result = write_flash(
            clock,
            devices,
            board,
            &loader_link,
            &loader_flash,
            &image,
            &product,
            usb.as_deref(),
        );
        return result;
    }

    // Resume: the device is already sitting in its update loader (the user pressed
    // Flash on the loader's row). Step 2 straight away.
    publish(flash, board, "checking the update loader…", None);
    let loader_model = current_model.as_deref().map(strip_ota);
    if !loader_model.is_some_and(|m| same_model(m, model)) {
        return Err(format!(
            "the loader is for another device than {product}: nothing was written"
        ));
    }
    write_flash(clock, devices, board, link, flash, &image, &product, usb.as_deref())
}

/// Step 2 on the loader: pull the whole image into the flash, then wait for the device
/// to reboot into the new firmware and verify its identity (the hub waits 40 s).
pub(crate) fn write_flash(
    clock: &Clock,
    devices: &Mutex<Vec<Device>>,
    board: &Board,
    link: &DeviceLink,
    flash: &Mutex<Option<FlashState>>,
    image: &[u8],
    product: &str,
    usb: Option<&str>,
) -> Result<String, String> {
    publish(flash, board, "sending the UPGRADE command…", None);
    link.send(&UPGRADE);
    thread::sleep(Duration::from_secs(2));
    publish(flash, board, "writing the flash…", None);
    let served = serve(link, image, &[FINISH_WRITE], Duration::from_secs(180), flash, board)?;
    if served.finish != Some(FINISH_WRITE) {
        return Err(format!(
            "the loader stopped after {} requests: replug and press Flash again to resume",
            served.count
        ));
    }
    // The write is done; the device reboots into the new firmware and comes back as a
    // new port. Wait for it and verify the identity the handshake reports.
    publish(flash, board, "written: waiting for the device to come back…", None);
    let back = wait_for_device(devices, clock, board, Duration::from_secs(40), |device| {
        on_usb(device, usb)
            && device
                .identity
                .lock()
                .ok()
                .and_then(|i| i.clone())
                .is_some_and(|text| !text.to_lowercase().starts_with("ota-"))
    });
    match back {
        Some((_, back_identity, back_flash, back_outcome)) => {
            // The claim protected the returning device while it was being verified;
            // it is not flashing, so release the row.
            if let Ok(mut state) = back_flash.lock() {
                *state = None;
            }
            let text = back_identity.lock().ok().and_then(|i| i.clone());
            if text.as_deref().is_some_and(|t| same_identity(t, product)) {
                if let Ok(mut outcome) = back_outcome.lock() {
                    *outcome = Some(format!("flashed: {}", friendly(product)));
                }
                Ok(product.to_string())
            } else {
                Err(format!(
                    "the device came back as {}, not {product}: the update may have failed",
                    text.unwrap_or_else(|| "nothing the handshake could read".to_string())
                ))
            }
        }
        None => Err(format!(
            "the device did not come back within 40 s: it should be running {product} — \
             check its row, and replug if it stays dark"
        )),
    }
}

pub(crate) struct Served {
    pub(crate) count: u32,
    /// The finish address the device asked for, if it asked for one of the listed
    /// ones before going idle.
    pub(crate) finish: Option<u32>,
}

/// Answer the device's read requests until it asks for one of the `finish` addresses
/// (both finish addresses are always acknowledged), publishing the served fraction of
/// the image to the row and the board.
pub(crate) fn serve(
    link: &DeviceLink,
    image: &[u8],
    finish: &[u32],
    idle: Duration,
    flash: &Mutex<Option<FlashState>>,
    board: &Board,
) -> Result<Served, String> {
    let mut count = 0u32;
    let mut served_bytes = 0usize;
    let mut last_pct = u16::MAX;
    let mut last = Instant::now();
    loop {
        let Some(pkt) = link.read(Duration::from_secs(1)) else {
            if last.elapsed() > idle {
                return Ok(Served { count, finish: None });
            }
            continue;
        };
        let Some(request) = parse_request(&pkt) else {
            continue;
        };
        last = Instant::now();
        if request.addr == FINISH_STAGE || request.addr == FINISH_WRITE {
            link.send(&response(request.addr, b"success\0", 0));
            if finish.contains(&request.addr) {
                return Ok(Served { count, finish: Some(request.addr) });
            }
            continue;
        }
        let len = request.len as usize;
        let start = request.addr as usize;
        if len > MAXDATA || start + len > image.len() {
            return Err(format!("bad request {:x}+{len}", request.addr));
        }
        // The hub's pacing: let the device settle before each packet.
        thread::sleep(Duration::from_millis(10));
        if !link.send(&response(request.addr, &image[start..start + len], request.fl)) {
            return Err("the device was disconnected mid-update".to_string());
        }
        count += 1;
        served_bytes += len;
        // Publish on whole percent steps only; the pane repaints anyway.
        let pct = (served_bytes * 100 / image.len().max(1)) as u16;
        if pct != last_pct {
            last_pct = pct;
            publish_progress(flash, board, (pct as f32 / 100.0).min(1.0));
        }
    }
}
