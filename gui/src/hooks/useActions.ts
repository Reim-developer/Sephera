/**
 * Layer 4: the actions a component can trigger.
 *
 * Extracted from `useAnalysis` because the layering needs a hook *layer* rather
 * than one file that mixes selectors and actions. `platform/commands.ts` imports
 * this instead of importing the store, which is what keeps `platform/` below
 * `state/` -- a command drives the store, not the other way round.
 *
 * Returning the whole surface rather than one selector per field is deliberate:
 * a component that destructures three actions would otherwise subscribe three
 * times, and Zustand re-renders on each.
 */

import { useClient, type ViewId } from "@/state/store";

/** Every action the store exposes. */
export interface ClientActions {
  setRoot: (path: string) => void;
  setIgnore: (patterns: readonly string[]) => void;
  setTarget: (path: string) => void;
  setSelected: (path: string | null) => void;
  setView: (view: ViewId) => void;
  recompute: () => void;
  recomputeAll: () => void;
  setTreeLevel: (directory: string, nodes: Parameters<
    ReturnType<typeof useClient.getState>["setTreeLevel"]
  >[1]) => void;
  clearTree: () => void;
}

/** The store's action surface, for a component that triggers work.
 *
 * The object literal is re-created on each render and the fields are stable, so
 * Zustand's equality check on the returned reference would re-render on every
 * parent change. Reading the getter instead returns the store's own bound
 * functions, which never change identity. */
export function useActions() {
  return useClient.getState();
}
