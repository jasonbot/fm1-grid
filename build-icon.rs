// Shared by the binary crates' build scripts. Windows gets the icon as a PE
// resource; other targets do not need it.
fn embed_windows_icon() {
    println!("cargo:rerun-if-changed=../logo.svg");
    println!("cargo:rerun-if-changed=../logo.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let svg = manifest.join("../logo.svg");
    let committed = manifest.join("../logo.ico");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let generated = out_dir.join("logo.ico");

    let icon = match rasterize(&svg, &generated) {
        Ok(()) => generated,
        Err(message) => {
            println!("cargo:warning={message}; using the committed logo.ico");
            committed
        }
    };

    if !icon.is_file() {
        println!("cargo:warning=no Windows icon at {}", icon.display());
        return;
    }

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(icon.to_str().expect("Windows icon path is UTF-8"));
    resource
        .compile()
        .expect("failed to embed the Windows executable icon");
}

fn rasterize(svg: &std::path::Path, output: &std::path::Path) -> Result<(), String> {
    if !svg.is_file() {
        return Err(format!("{} is missing", svg.display()));
    }
    let status = std::process::Command::new("magick")
        .arg("-background")
        .arg("none")
        .arg(svg)
        .arg("-define")
        .arg("icon:auto-resize=256,128,64,48,32,24,20,16")
        .arg(output)
        .status()
        .map_err(|error| format!("could not run magick: {error}"))?;
    if !status.success() {
        return Err(format!("magick failed to rasterize logo.svg ({status})"));
    }
    if !output.is_file() {
        return Err("magick did not write logo.ico".into());
    }
    Ok(())
}
