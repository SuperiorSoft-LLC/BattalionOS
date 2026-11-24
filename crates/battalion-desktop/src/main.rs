// SPDX-License-Identifier: MPL-2.0
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "gui")]
fn main() {
    // An app worker (this same program, started by the OS with an internal argument) runs one app's
    // library and nothing else: before any window code.
    if let Some(code) = battalion_host::worker::run_if_worker() {
        std::process::exit(code);
    }
    use battalion_desktop::{commands::*, host::Host};
    use battalion_host::Paths;
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    // A stale WebView2 registration (Windows Sandbox's image has one) would leave the window blank white.
    if let Some(dir) = battalion_desktop::webview2::apply_fallback() {
        eprintln!("webview2: registered runtime folder is missing; using {}", dir.display());
    }
    let paths = Paths::from_env().unwrap_or_else(|| Paths::new(std::env::temp_dir().join("battalion-os")));
    let _ = paths.ensure();
    // The installer's first-run.json may name the catalog; seed it before the Armory reads os.json.
    if let Some(plan) = battalion_host::firstrun::plan_path_beside_exe() {
        let _ = battalion_host::firstrun::seed_sources(&paths, &plan);
    }
    let host = Host::new(paths);
    // Apps belong to the run that started them; a leftover of an older run is stopped by its recorded pid.
    for (id, r) in host.sup.recover_all() {
        if let Err(e) = r {
            eprintln!("recover {id}: {e:?}");
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
        // Closing the window hides it: apps and agents keep working. A new start (single instance) or
        // the next launch shows it again. Exit (os_exit) is the only way the OS ends.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if battalion_desktop::access::close_hides_window(window.label()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
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
    app.run(|handle, ev| match ev {
        // No window left is not an exit: the OS runs on for its apps until Exit.
        tauri::RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
        // Exit: every app is stopped cleanly (its stop, the timeout, then its job); then the OS goes.
        tauri::RunEvent::Exit => {
            if let Some(h) = handle.try_state::<Host>() {
                for (id, e) in h.armory.exit_os() {
                    eprintln!("stop {id} at exit: {e}");
                }
            }
        }
        _ => {}
    });
}

#[cfg(not(feature = "gui"))]
fn main() {
    if let Some(code) = battalion_host::worker::run_if_worker() {
        std::process::exit(code);
    }
    eprintln!("battalion-desktop was built without the gui feature");
}
