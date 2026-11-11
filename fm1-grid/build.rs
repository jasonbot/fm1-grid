// The executable icon is rasterized from logo.svg. Regenerate logo.ico when the
// logo changes; Windows uses the largest entry for the shell and small sizes for
// the title bar and task switcher.
fn main() {
    println!("cargo:rerun-if-changed=../logo.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let icon = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../logo.ico");
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(icon.to_str().expect("logo.ico path is UTF-8"));
    resource
        .compile()
        .expect("failed to embed the Windows executable icon");
}
