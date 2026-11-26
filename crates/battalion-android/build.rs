// SPDX-License-Identifier: MPL-2.0
//! The os_* command list and the window capabilities are the desktop's own (one source): the list is read from
//! battalion-desktop's build.rs, the capabilities from its folder.
fn main() {
    let desk = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../battalion-desktop");
    let text = std::fs::read_to_string(desk.join("build.rs")).expect("battalion-desktop/build.rs");
    println!("cargo:rerun-if-changed={}", desk.join("build.rs").display());
    let list = text.split("const COMMANDS: &[&str] = &[").nth(1).and_then(|r| r.split("];").next()).expect("COMMANDS list in battalion-desktop/build.rs");
    let commands: Vec<&'static str> = list.split(',').map(|c| c.trim().trim_matches('"')).filter(|c| !c.is_empty()).map(|c| &*Box::leak(c.to_string().into_boxed_str())).collect();
    let caps = format!("{}/capabilities/*.json", desk.display()).replace('\\', "/");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .capabilities_path_pattern(Box::leak(caps.into_boxed_str()))
            .app_manifest(tauri_build::AppManifest::new().commands(Box::leak(commands.into_boxed_slice()))),
    )
    .expect("tauri build");
}
