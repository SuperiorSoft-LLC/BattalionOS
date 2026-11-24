// SPDX-License-Identifier: MPL-2.0
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "gui")]
fn main() {
    use battalion_desktop::{commands::*, host::Host};
    use battalion_host::Paths;
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    let paths = Paths::from_env().unwrap_or_else(|| Paths::new(std::env::temp_dir().join("battalion-os")));
    let _ = paths.ensure();
    // The installer's first-run.json may name the catalog; seed it before the Armory reads os.json.
    if let Some(plan) = battalion_host::firstrun::plan_path_beside_exe() {
        let _ = battalion_host::firstrun::seed_sources(&paths, &plan);
    }
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
            os_badge,
            os_show_desktop,
            os_catalog,
            os_install_app,
            os_open_login,
            os_login_done,
            os_update_app,
            os_rollback_app,
            os_remove_app,
            os_exit,
            os_first_run_retry
        ])
        .setup(|app| {
            // Armory progress goes to every window as an event; the shell shows it.
            let handle = app.handle().clone();
            let sink: battalion_host::armory::ProgressSink = std::sync::Arc::new(move |p| {
                let _ = tauri::Emitter::emit(&handle, battalion_desktop::events::ARMORY_PROGRESS, p);
            });
            app.state::<Host>().armory.set_progress_sink(Some(sink));
            // First open: install what the installer asked for (its checkbox), in the background, with progress events.
            let h2 = app.handle().clone();
            std::thread::spawn(move || {
                let host = h2.state::<Host>();
                battalion_desktop::host::run_first_run(&h2, &host);
            });
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("BattalionOS")
                .inner_size(1200.0, 800.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build battalion-desktop");
    app.run(|handle, ev| {
        // The OS exits: apps keep running (the next start adopts them) unless their manifest says `onOsExit: stop`.
        if let tauri::RunEvent::Exit = ev {
            if let Some(h) = handle.try_state::<Host>() {
                for (id, e) in h.armory.exit_os() {
                    eprintln!("stop {id} at exit: {e}");
                }
            }
        }
    });
}

#[cfg(not(feature = "gui"))]
fn main() {
    eprintln!("battalion-desktop was built without the gui feature");
}
