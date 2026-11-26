// SPDX-License-Identifier: MPL-2.0
//! BattalionOS on Android (AND-1). One signed Android app that is the OS: the same shell page, Armory and app state
//! machine as the desktop (battalion-desktop's commands). What differs is only what a phone has:
//! - an app runs as its page (the page runner in battalion-host); the Armory refuses native content and `init`;
//! - one window (the shell); app pages open in its app area, as on the desktop;
//! - no tray, no single-instance plugin, no Start menu shortcuts, and no self update (the OS updates by its store).

use battalion_desktop::{commands::*, host::Host};
use battalion_host::Paths;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
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
        .setup(|app| {
            // The OS home is the app's own private storage (`<data>/os`); nothing outside it.
            let paths = Paths::new(app.path().app_data_dir()?.join("os"));
            paths.ensure()?;
            // AND-2: the same public catalog as the Windows installer's first-run.json (data, carried in the build; a
            // phone has no folder beside an exe). Seeded only while os.json names no source; a source set by hand stays.
            const PLAN: &str = include_str!("../../../packaging/first-run.json");
            let sources = battalion_host::firstrun::plan_sources(PLAN);
            if let Err(e) = battalion_host::firstrun::seed_source_list(&paths, &sources) {
                eprintln!("catalog sources: {e}");
            }
            // BOS-DEFAULT-INF: the plan's apps are installed on the first open, as on the desktop (once; a removed app stays removed).
            app.manage(Host::new(paths).with_first_run_plan(PLAN));
            for (id, r) in app.state::<Host>().sup.recover_all() {
                if let Err(e) = r {
                    eprintln!("recover {id}: {e:?}");
                }
            }
            let handle = app.handle().clone();
            let sink: battalion_host::armory::ProgressSink = std::sync::Arc::new(move |p| {
                let _ = tauri::Emitter::emit(&handle, battalion_desktop::events::ARMORY_PROGRESS, p);
            });
            app.state::<Host>().armory.set_progress_sink(Some(sink));
            let h2 = app.handle().clone();
            std::thread::spawn(move || {
                battalion_desktop::host::run_first_run(&h2, &h2.state::<Host>());
            });
            WebviewWindowBuilder::new(app, battalion_desktop::access::MAIN, WebviewUrl::App("index.html".into())).build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("run BattalionOS");
}
