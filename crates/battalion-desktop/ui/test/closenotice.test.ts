// SPDX-License-Identifier: MPL-2.0
import { describe, expect, it } from "vitest";
import { closeAsks, closeNoticeSeen, CLOSE_NOTICE_TEXT } from "../src/closeNotice";

function mem() {
  const m = new Map<string, string>();
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
}

describe("close notice", () => {
  it("asks the first time only", () => {
    const s = mem();
    expect(closeAsks(s)).toBe(true);
    closeNoticeSeen(s);
    expect(closeAsks(s)).toBe(false);
  });
  it("says the OS keeps running and names Exit, no app", () => {
    expect(CLOSE_NOTICE_TEXT).toMatch(/keep running/);
    expect(CLOSE_NOTICE_TEXT).toMatch(/Exit/);
  });
});
