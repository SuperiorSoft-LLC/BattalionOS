// SPDX-License-Identifier: MPL-2.0
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "gui")]
fn main() {
    use battalion_desktop::{commands::*, host::Host};
    use battalion_host::Paths;
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    let paths = Paths::from_env().unwrap_or_else(|| Paths::new(std::env::temp_dir().join("battalion-os")));
    let _ = paths.ensure();
    let host = Host::new(paths);
    for (id, r) in host.sup.adopt_all() {
        if let Err(e) = r {
            eprintln!("adopt {id}: {e:?}");
        }
    }
    let app = tauri::Builder::default()
        // Single instance: a second launch focuses the first one's main window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .manage(host)
        .invoke_handler(tauri::generate_handler![
            os_window_info,
            os_list_apps,
            os_app_status,
            os_app_health,
            os_start_app,
            os_stop_app,
            os_open_app_window,
            os_close_app_window,
            os_catalog,
            os_install_app,
            os_open_login,
            os_login_done
        ])
        .setup(|app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("BattalionOS")
                .inner_size(1200.0, 800.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build battalion-desktop");
    app.run(|handle, ev| {
        // The OS exits: let go of the apps (they keep running per their manifest), do not stop them.
        if let tauri::RunEvent::Exit = ev {
            if let Some(h) = handle.try_state::<Host>() {
                h.sup.release_all();
            }
        }
    });
}

#[cfg(not(feature = "gui"))]
fn main() {
    eprintln!("battalion-desktop was built without the gui feature");
}
