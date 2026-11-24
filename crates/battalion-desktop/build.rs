// SPDX-License-Identifier: MPL-2.0
/// Every os_* command. Kept in step with `commands.rs` by tests/commands_frozen.rs.
#[cfg(feature = "gui")]
const COMMANDS: &[&str] = &[
    "os_window_info",
    "os_list_apps",
    "os_app_status",
    "os_app_health",
    "os_start_app",
    "os_stop_app",
    "os_open_app_window",
    "os_close_app_window",
    "os_catalog",
    "os_install_app",
    "os_open_login",
    "os_login_done",
];

fn main() {
    #[cfg(feature = "gui")]
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)))
        .expect("tauri build");
}
