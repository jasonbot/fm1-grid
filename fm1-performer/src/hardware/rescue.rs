//! The rescue flash: flashing a MIDI port the scanner never registered — a unit
//! bricked badly enough to enumerate under a name the probe filter rejects, so it
//! never becomes a device row.
//!
//! Unlike the normal flow there is no handshake, no registry and no clock: the
//! UPGRADE command goes to the port as it is, and the two reboots (into the update
//! loader, then into the new firmware) are tracked by watching the port list itself.
//! While a rescue runs the scanner registers no new devices (see `scan`), so no
//! clock bytes or graph routes reach the port mid-update. All state lands on the
//! pane-level flash board; a rescue target has no row.
//!
//! A child module of `hardware`: the update protocol (`serve`, `open_link`, `probe`,
//! the package helpers and the protocol constants) stays private to the parent.

use super::*;
use std::time::{Duration, Instant};

/// Whether a port name is one of this process's own MIDI clients ("fm1-grid*") —
/// never a flash target.
pub(super) fn ours(name: &str) -> bool {
    name.starts_with("fm1-grid")
}

/// Every MIDI port on the system right now, minus our own clients.
fn port_names() -> Vec<String> {
    let Ok(input) = MidiInput::new("fm1-grid-scan") else {
        return Vec::new();
    };
    input
        .ports()
        .iter()
        .filter_map(|port| input.port_name(port).ok())
        .filter(|name| !ours(name))
        .collect()
}

/// Poll the port list for a port that was not there when the wait started: a unit
/// rebooting through its update loader vanishes and comes back, possibly under
/// another name. A port that disappeared mid-wait and reappears counts.
fn wait_new_port(timeout: Duration) -> Option<String> {
    let baseline = port_names();
    let mut missing: Vec<String> = Vec::new();
    let deadline = Instant::now() + timeout;
    loop {
        let now = port_names();
        for name in &baseline {
            if !now.contains(name) && !missing.contains(name) {
                missing.push(name.clone());
            }
        }
        if let Some(name) = now
            .iter()
            .find(|name| !baseline.contains(*name) || missing.contains(*name))
        {
            return Some(name.clone());
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

/// The rescue flash on its own thread: the flow, then the board's outcome. The scan
/// loop clears the busy flag when this returns. A success also remembers the unit's
/// come-back port name as a probe needle — the scanner recognizes the unit from now
/// on (see [`remember_probe_name`]).
pub(super) fn rescue_task(
    name: &str,
    bytes: Vec<u8>,
    board: &Board,
    router: &Routing,
    store: Option<&Store>,
    extra_probes: &Mutex<Vec<String>>,
) {
    let result = rescue_flow(name, bytes, board, router);
    if let Ok((_, back_name)) = &result {
        remember_probe_name(store, extra_probes, back_name);
    }
    settle_board(
        board,
        &result
            .as_ref()
            .map(|(product, _)| product.clone())
            .map_err(|e| e.clone()),
    );
}

/// The flow itself. The port may be in either update phase: a running firmware pulls
/// the image up to the staging address and reboots into its loader; a unit already
/// sitting in its loader pulls straight through to the write address. One serve with
/// both finish addresses tells the two apart. On success: the product string and the
/// name of the port the unit came back on (the caller remembers it as a probe
/// needle).
fn rescue_flow(
    name: &str,
    bytes: Vec<u8>,
    board: &Board,
    router: &Routing,
) -> Result<(String, String), String> {
    // No row exists for an unregistered port; this throwaway slot lets publish()
    // and serve() run unchanged — every phase lands on the board only.
    let flash: Mutex<Option<FlashState>> = Mutex::new(None);
    let Some(product) = product_of(&bytes) else {
        return Err("not an FM-1 package: no product identity in the file".to_string());
    };
    let image = logical_image(&bytes);
    let Some(link) = open_link(name, router) else {
        return Err(format!("could not open {name}: another program has the port"));
    };
    publish(&flash, board, "sending the UPGRADE command…", None);
    link.send(&UPGRADE);
    thread::sleep(Duration::from_secs(2));
    publish(&flash, board, "waiting for the device to pull the update…", None);
    let served = serve(
        &link,
        &image,
        &[FINISH_STAGE, FINISH_WRITE],
        Duration::from_secs(8),
        &flash,
        board,
    )?;
    match served.finish {
        // The port was already sitting in the update loader: the serve above ran
        // straight through the write. Nothing more to send.
        Some(FINISH_WRITE) => {}
        Some(FINISH_STAGE) => {
            publish(&flash, board, "step 1 done: waiting for the update loader…", None);
            let Some(loader_name) = wait_new_port(Duration::from_secs(30)) else {
                return Err(
                    "the update loader did not appear within 30 s: replug the USB cable \
                     and press Flash again — the device stays in update mode until the \
                     update is finished"
                        .to_string(),
                );
            };
            let Some(loader) = open_link(&loader_name, router) else {
                return Err(format!(
                    "the update loader ({loader_name}) could not be opened: replug and \
                     press Flash again to resume"
                ));
            };
            publish(&flash, board, "sending the UPGRADE command…", None);
            loader.send(&UPGRADE);
            thread::sleep(Duration::from_secs(2));
            publish(&flash, board, "writing the flash…", None);
            let served = serve(
                &loader,
                &image,
                &[FINISH_WRITE],
                Duration::from_secs(180),
                &flash,
                board,
            )?;
            if served.finish != Some(FINISH_WRITE) {
                return Err(format!(
                    "the loader stopped after {} requests: replug and press Flash again \
                     to resume",
                    served.count
                ));
            }
        }
        // serve() only returns the finish addresses it was given; both are covered.
        Some(addr) => {
            return Err(format!(
                "the device asked for an unexpected finish address {addr:#010x}: \
                 nothing was written"
            ));
        }
        None if served.count == 0 => {
            return Err(format!(
                "no update requests arrived on {name}: it is not an FM-1 in update \
                 mode — nothing was written"
            ));
        }
        None => {
            return Err(format!(
                "the device stopped after {} requests: nothing was written",
                served.count
            ));
        }
    }
    // The write is done; the unit reboots into the new firmware and comes back as a
    // new (or reappearing) port. The handshake verifies it — both the loader and a
    // healthy firmware answer it, so a silent port means the write did not take.
    publish(&flash, board, "written: waiting for the device to come back…", None);
    let Some(back_name) = wait_new_port(Duration::from_secs(40)) else {
        return Err(format!(
            "the device did not come back within 40 s: it should be running {product} — \
             replug if it stays dark"
        ));
    };
    let text = open_link(&back_name, router).and_then(|link| probe(&link));
    match text.as_deref() {
        Some(text) if text == product => Ok((product.to_string(), back_name)),
        Some(text) => Err(format!(
            "the device came back as {text}, not {product}: the update may have failed"
        )),
        None => Err(format!(
            "the device on {back_name} does not answer the handshake: the write \
             finished, but the firmware cannot be verified"
        )),
    }
}
