// SPDX-License-Identifier: Apache-2.0
// Copyright (c) SuperiorSoft LLC
// APP-API-1: typed errors. The OS answers a refused call with a message that ends in its stable code, `... [leaves_root]`
// (docs/commands.md). `toError` turns that message into one of these classes, so an app writes `if (e instanceof NotFoundError)`.

/** Every error of an OS call. `code` is the stable name (the schema's FsErrorCode / StoreErrorCode, or one of the client's). */
export class BosError extends Error {
  readonly code: string;
  readonly call: string;
  constructor(message: string, code: string, call: string) {
    super(message);
    this.name = "BosError";
    this.code = code;
    this.call = call;
  }
}

/** The app's app.json lacks the permission the call needs (the message names it, e.g. `files.read`). */
export class PermissionError extends BosError {
  readonly permission: string | null;
  constructor(message: string, call: string, permission: string | null) {
    super(message, "permission", call);
    this.name = "PermissionError";
    this.permission = permission;
  }
}
/** A path that would leave the OS root (`..`, an absolute path, a link out). */
export class LeavesRootError extends BosError {
  constructor(m: string, call: string) { super(m, "leaves_root", call); this.name = "LeavesRootError"; }
}
export class NotFoundError extends BosError {
  constructor(m: string, call: string) { super(m, "not_found", call); this.name = "NotFoundError"; }
}
/** The name is taken (write with `overwrite: false`, mkdir, rename, copy). */
export class ExistsError extends BosError {
  constructor(m: string, call: string) { super(m, "exists", call); this.name = "ExistsError"; }
}
/** Over a limit the OS settings set (`fs.writeMaxBytes`, `store.maxBytes`, ...). */
export class TooLargeError extends BosError {
  constructor(m: string, call: string) { super(m, "too_large", call); this.name = "TooLargeError"; }
}
/** Wrong or missing arguments. */
export class BadArgsError extends BosError {
  constructor(m: string, call: string) { super(m, "bad_args", call); this.name = "BadArgsError"; }
}
/** The call is not served here (this device, this OS version, or the local backend). */
export class NotAvailableError extends BosError {
  constructor(m: string, call: string) { super(m, "not_available", call); this.name = "NotAvailableError"; }
}
/** No answer from the OS in time. */
export class TimeoutError extends BosError {
  constructor(m: string, call: string) { super(m, "timeout", call); this.name = "TimeoutError"; }
}

const BY_CODE: Record<string, new (m: string, call: string) => BosError> = {
  leaves_root: LeavesRootError,
  not_found: NotFoundError,
  exists: ExistsError,
  too_large: TooLargeError,
  bad_args: BadArgsError,
};

/** The OS's refusal text for `call` as a typed error. */
export function toError(call: string, raw: unknown): BosError {
  if (raw instanceof BosError) return raw;
  const text = raw instanceof Error ? raw.message : typeof raw === "string" ? raw : "the OS refused the call";
  const perm = /has no '([^']+)' permission/.exec(text);
  if (perm) return new PermissionError(text, call, perm[1] ?? null);
  if (/not available|is not available to an app's page|not on a phone yet/.test(text)) return new NotAvailableError(text, call);
  if (/no answer from the OS/.test(text)) return new TimeoutError(text, call);
  const m = /\s*\[([a-z_]+)\]\s*$/.exec(text);
  if (!m) return new BosError(text, "unknown", call);
  const code = m[1]!;
  const message = text.slice(0, m.index);
  const Cls = BY_CODE[code];
  return Cls ? new Cls(message, call) : new BosError(message, code, call);
}
