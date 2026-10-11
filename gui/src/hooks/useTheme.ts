/**
 * Layer 4: the theme, as state the client can read and switch.
 *
 * A hook rather than a field on the store, because the theme is a property of the
 * document and not of the analysis: the store holds what is being counted, and
 * a field that changes how the window is painted is a field that would re-render
 * every view on a theme change. This way only the toggle re-renders.
 */

import { useCallback, useEffect, useState } from "react";

import type { ThemeId } from "@/lib/ipc";
import { THEMES, applyTheme, rememberTheme } from "@/services/theme";

/** The theme, and the switch that changes it. */
export interface Theme {
  /** Which theme the document is painted in. */
  theme: ThemeId;
  /** Change to `theme`, painting the document and remembering the choice. */
  setTheme: (theme: ThemeId) => void;
  /** Move to the other theme. */
  toggle: () => void;
}

/** The theme, and the switch that changes it.
 *
 * The first value is read from where the host kept it, and the document is
 * painted as a side effect rather than in the render itself: a render that
 * touches the DOM is a render that can be interrupted halfway through and leave
 * the document in a state React does not know about. */
export function useTheme(): Theme {
  const [theme, setTheme] = useState<ThemeId>(() => rememberTheme());

  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  // The system's preference is watched, and only while the reader has not
  // chosen. A stored choice is an answer, and following the system after an
  // answer is how a toggle comes to feel broken.
  useEffect(() => {
    const stored = localStorage.getItem("sephera.theme");
    if (stored === "light" || stored === "dark") return;

    const query = globalThis.matchMedia?.("(prefers-color-scheme: light)");
    if (!query) return;
    const change = (event: MediaQueryListEvent) =>
      setTheme(event.matches ? "light" : "dark");

    query.addEventListener("change", change);
    return () => query.removeEventListener("change", change);
  }, []);

  const toggle = useCallback(() => {
    setTheme((current) => {
      const at = THEMES.indexOf(current);
      return THEMES[(at + 1) % THEMES.length];
    });
  }, []);

  return { theme, setTheme, toggle };
}
