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
    // BOS-31, developer builds only: `--dev-app <folder> [--dev-url <loopback url>]` (repeatable) takes an app from its folder.
    #[cfg(feature = "dev")]
    let dev_ids = battalion_desktop::devmode::register_from_args(&paths, &std::env::args().skip(1).collect::<Vec<_>>());
    let host = Host::new(paths);
    // Started with `--open <app-id>` (a shortcut, the command line): the shell opens that app when it has loaded.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    *host.open_request.lock().unwrap() = battalion_desktop::launch::open_request(&argv);
    // BOS-34: a second start is the process Windows lets take the foreground; it hands that right on before the
    // single-instance plugin signals the running OS, so the running one may bring its window to the front.
    battalion_desktop::front::allow_any_to_take_foreground();
    let app = tauri::Builder::default()
        // Single instance: a second launch focuses the first one's main window.
        // `--open <app-id>` of that second start is handed to this run (BOS-19); the rest of its line is ignored.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            battalion_desktop::front::bring_main_to_front(app);
            if let Some(id) = battalion_desktop::launch::open_request(&args[args.len().min(1)..]) {
                *app.state::<Host>().open_request.lock().unwrap_or_else(|p| p.into_inner()) = Some(id);
                let _ = tauri::Emitter::emit(app, battalion_desktop::events::OPEN_REQUEST, ());
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
            os_first_run_retry,
            os_take_first_run_notice,
            os_app_page,
            os_frame_badge,
            os_frame_pick_folder,
            os_frame_pick_file,
            os_frame_phone_call,
            os_frame_open_external,
            os_frame_login_open,
            os_frame_login_close,
            os_take_open_request,
            os_update_state,
            os_update_check,
            os_update_install
        ])
        .setup(move |app| {
            #[cfg(feature = "dev")]
            battalion_desktop::devmode::start(app.handle(), &dev_ids);
            // Apps belong to the run that started them; a leftover of an older run is stopped by its recorded pid. This runs only
            // here, in the first instance: a second start is handed to the running one by the single-instance plugin before setup and
            // must never stop that one's apps (BOS-19).
            for (id, r) in app.state::<Host>().sup.recover_all() {
                if let Err(e) = r {
                    eprintln!("recover {id}: {e:?}");
                }
            }
            // Armory progress goes to every window as an event; the shell shows it.
            let handle = app.handle().clone();
            let sink: battalion_host::armory::ProgressSink = std::sync::Arc::new(move |p| {
                let _ = tauri::Emitter::emit(&handle, battalion_desktop::events::ARMORY_PROGRESS, p);
            });
            app.state::<Host>().armory.set_progress_sink(Some(sink));
            // BOS-30: an OS update stopped the apps and wrote the list of what ran; this start (the new version, or the old one
            // after an installer that was cancelled) starts them again. Then the OS checks its own feed, and once a day after that.
            let h3 = app.handle().clone();
            std::thread::spawn(move || {
                let host = h3.state::<Host>();
                for (id, e) in host.armory.os_resume_apps() {
                    eprintln!("start {id} after the update: {e}");
                }
                loop {
                    tauri::async_runtime::block_on(battalion_desktop::commands::check_os_update(&h3, &host));
                    std::thread::sleep(std::time::Duration::from_secs(battalion_desktop::osupdate::CHECK_EVERY_SECS));
                }
            });
            // First open: install what the installer asked for (its checkbox), in the background, with progress events.
            let h2 = app.handle().clone();
            std::thread::spawn(move || {
                let host = h2.state::<Host>();
                battalion_desktop::host::run_first_run(&h2, &host);
                // Start menu: one shortcut per installed app (after first run, which may have installed some).
                tauri::async_runtime::block_on(battalion_desktop::commands::sync_shortcuts(&host));
            });
            let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title(if battalion_host::dev::ENABLED { "BattalionOS (dev)" } else { "BattalionOS" })
                .inner_size(1200.0, 800.0)
                // BOS-27: no Windows title bar; the shell draws the bar (the OS's own, or the app's when it has one).
                .decorations(false)
                ;
            // BOS-31, developer builds: the OS window's own web data, inside the dev data folder (never beside the exe or in a shared place).
            #[cfg(feature = "dev")]
            let win = win.data_directory(app.state::<Host>().paths.root().join("webview-os"));
            win.build()?;
            // BOS-34: the notification area icon while the OS runs: Open (the window, in front) and Exit (as Start > Exit).
            battalion_desktop::front::tray(app.handle())?;
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
