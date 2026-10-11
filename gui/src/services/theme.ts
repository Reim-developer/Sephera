/**
 * Layer 1: the theme, which the workbench is painted in.
 *
 * A service rather than something the store does, for the reason the dialog is a
 * service: the theme is a capability of the host (`localStorage`, and the system
 * preference the webview can see), not a piece of application state. The store
 * holds which theme is *chosen*; this holds where that choice is kept and how the
 * document is painted.
 *
 * The theme is applied by setting `data-theme` on `<html>`, and every colour in
 * `tokens.scss` is a custom property keyed off it. That is why the toggle does
 * not re-render anything: the browser changes the value and every rule that read
 * `var(--editor-bg)` follows, with React in neither path.
 */

import type { ThemeId } from "@/lib/ipc";

/** What the theme service answers.
 *
 * A record of functions rather than one object, so the registry can hold it the
 * same way it holds the dialog service, and a test can replace it wholesale. */
export const themeService = {
  /** What the theme is, and why: a stored choice, the system's, or the default. */
  initial: rememberTheme,
  /** Paint the document in `theme` and remember it. */
  apply: applyTheme,
  /** What the system is set to, for a reader who has not chosen. */
  system: systemTheme,
};

/** Where the choice is kept, and what it is called. */
const STORAGE_KEY = "sephera.theme";

/** The themes the toggle cycles through, in the order it shows them. */
export const THEMES: readonly ThemeId[] = ["dark", "light"];

/** What the system is set to, when the reader can be asked.
 *
 * `matchMedia` is absent in a webview without a matchMedia implementation of its
 * own, and asking a question that returns nothing is how a theme comes back
 * `undefined`. So the default is the dark theme, which is what this application
 * was designed against and what a reader who has not chosen will see. */
function systemTheme(): ThemeId {
  const query = globalThis.matchMedia?.("(prefers-color-scheme: light)");
  return query?.matches ? "light" : "dark";
}

/**
 * The theme to start in.
 *
 * A stored choice wins: it is an answer, and second-guessing an answer is how a
 * toggle comes to feel broken. Without one the system's preference is read, and
 * without a system preference it is the dark theme.
 */
export function rememberTheme(): ThemeId {
  const stored = localStorage.getItem(STORAGE_KEY);
  // Anything that is not a theme name is treated as nothing stored. A value
  // written by a future version of the settings panel is not a value this one
  // should refuse to start over.
  if (stored === "light" || stored === "dark") return stored;
  return systemTheme();
}

/** Paint the document in `theme`.
 *
 * Also moves the reader's choice into storage, so the next start is the same
 * theme without having to ask the system again. */
export function applyTheme(theme: ThemeId): void {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // A webview with storage disabled is a webview that forgets. Nothing here
    // needs the write to succeed: the theme is already painted, and the only
    // thing lost is the memory of it.
  }
}
