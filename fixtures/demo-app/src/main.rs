// SPDX-License-Identifier: Apache-2.0
//! Test runner for the fixture: `demo-app '<context json>'` runs the library in this process, as the OS worker
//! would, and exits with its code. A line `stop` on stdin is the OS's stop call (`battalion_app_stop`).
//! Only tests/modes.rs uses it; the OS loads the library itself.

fn main() {
    std::thread::spawn(|| {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if line.trim() == "stop" {
                demo_app::battalion_app_stop();
            }
        }
    });
    let ctx = std::env::args().nth(1).unwrap_or_default();
    let c = std::ffi::CString::new(ctx).expect("context has no NUL");
    // SAFETY: a NUL-terminated string that lives for the call.
    std::process::exit(unsafe { demo_app::battalion_app_run(c.as_ptr()) });
}
