#[cfg(windows)]
fn main() {
    use std::{env, path::PathBuf};

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set"),
    );
    let icon = manifest_dir.join("assets").join("app.ico");

    println!("cargo:rerun-if-changed={}", icon.display());

    if !icon.is_file() {
        panic!("Windows application icon not found: {}", icon.display());
    }

    let icon = icon
        .to_str()
        .expect("Windows application icon path contains invalid UTF-8");

    let mut resource = winres::WindowsResource::new();
    resource.set_icon(icon);
    resource
        .compile()
        .expect("failed to embed Windows application icon");
}

#[cfg(not(windows))]
fn main() {}
