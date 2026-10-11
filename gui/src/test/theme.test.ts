/**
 * The theme, and what the toggle does to it.
 *
 * A theme is a `data-theme` attribute on the document and a value in storage, so
 * the tests drive those two and nothing else. The important cases are the ones
 * that read as broken to a user: a stored choice must survive, the system's
 * preference must be followed until it is overridden, and a value this version
 * did not write must be treated as no value at all rather than as a theme name.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  THEMES,
  applyTheme,
  rememberTheme,
} from "@/services/theme";

/** A `matchMedia` stub, since jsdom has one but its answers are not ours. */
function prefersLight(matches: boolean): void {
  vi.stubGlobal("matchMedia", (query: string) => ({
    matches: query.includes("light") ? matches : !matches,
    // The addEventListener the hook uses; jsdom's own does not need stubbing,
    // but a stub that does not implement it is a hook that fails to mount.
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

describe("the theme", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
    prefersLight(false);
  });

  it("paints the document in the theme it is given", () => {
    applyTheme("light");
    expect(document.documentElement.dataset.theme).toBe("light");

    applyTheme("dark");
    expect(document.documentElement.dataset.theme).toBe("dark");
  });

  it("remembers the choice, so the next start is the same", () => {
    applyTheme("light");
    expect(localStorage.getItem("sephera.theme")).toBe("light");
    expect(rememberTheme()).toBe("light");
  });

  it("follows the system when nothing has been chosen", () => {
    prefersLight(true);
    expect(rememberTheme()).toBe("light");
  });

  it("prefers a stored choice over the system", () => {
    applyTheme("dark");
    prefersLight(true);
    // The reader chose dark. A system set to light is a preference the reader
    // overrode, and second-guessing it is how a toggle feels broken.
    expect(rememberTheme()).toBe("dark");
  });

  it("treats a value it did not write as nothing stored", () => {
    localStorage.setItem("sephera.theme", "solarized");
    expect(rememberTheme()).toBe("dark");
  });

  it("cycles through both themes and comes back", () => {
    expect(THEMES).toEqual(["dark", "light"]);
    const at = THEMES.indexOf("dark");
    expect(THEMES[(at + 1) % THEMES.length]).toBe("light");
    const there = THEMES.indexOf("light");
    expect(THEMES[(there + 1) % THEMES.length]).toBe("dark");
  });

  it("does not throw when storage refuses a write", () => {
    // A webview with storage disabled. The theme is already painted, so the only
    // thing lost is the memory of it -- and a throw here would take the click
    // with it.
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("storage disabled");
    });
    expect(() => applyTheme("light")).not.toThrow();
    expect(document.documentElement.dataset.theme).toBe("light");
  });
});
