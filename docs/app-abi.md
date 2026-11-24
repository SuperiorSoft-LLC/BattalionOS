# App ABI v1

An app is a library, not a program. This page is the whole contract between the OS's worker and the library.
The Rust form is `battalion_app_api::abi` (Apache-2.0); a library in any language that exports these C symbols works.

```c
uint32_t battalion_abi_version(void);                 /* returns 1 */
int32_t  battalion_app_run(const char *context_json); /* blocks until the app ends; returns a code */
void     battalion_app_stop(void);                    /* asks run to return; called from another thread */
```

- **Package.** `app.json` -> `process.<target>.library` (`windows-x86_64` `.dll`, `macos-aarch64` `.dylib`, `linux-x86_64` `.so`),
  with the same `args`, `env`, `readyTimeoutMs`, `stopTimeoutMs` as before and optional `envScrub` (prefixes of the
  OS's own environment the app must not inherit; `BATTALION_*` is always removed). `exec` is refused with
  "process.<target>.exec is not allowed: an app ships a library ...".
- **Start.** The OS starts its own program as the app's worker (`<os> --bos-app-worker <library>`) as its child, puts it in
  its job, then hands it a handshake and the context on stdin. A worker started by hand has no handshake and refuses to run
  (exit code 2). The worker loads the library, checks `battalion_abi_version`, and calls `battalion_app_run` on a new
  thread (the app's own).
- **Context** (`run`'s argument, one JSON object, valid during the call): `contract`, `abi`, `appId`, `appVersion`,
  `appDir`, `stateDir`, `readyFile`, `tokenFile`, `args`. The contract environment (`BATTALION_*`) is also set in the worker.
- **Ready and health** are as in contract v1: write `readyFile` (loopback `url`), answer `GET health`.
- **Stop.** The OS closes the worker's stdin (also what happens when the OS itself ends). The worker calls
  `battalion_app_stop`; the app makes `run` return; the worker exits with `run`'s code. After `stopTimeoutMs` the OS ends the
  worker's job (the worker and everything the app started).
- **run returns by itself** = the app ended; the existing restart rules apply. A panic must not cross the ABI.
- **Ownership.** The worker is the OS's child. On Windows it is in a Job Object with KILL_ON_JOB_CLOSE: if the OS is killed,
  the worker and its grandchildren end with it. Elsewhere its stdin closes and its process group is ended on stop.
