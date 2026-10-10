/**
 * Layer 3 (cont): the hooks.
 *
 * A component subscribes to the store through a hook and calls a command. That is
 * the whole contract, and it means a component has no idea the service layer
 * exists -- it cannot, because it never imports it.
 *
 * The hooks are thin on purpose. Zustand selectors do the filtering; what is left
 * here is naming, so a component says `useAnalysis("loc")` rather than assembling
 * the same four-field selector at every call site.
 */

import { useClient, type Analysis, type ViewId } from "@/state/store";
import type {
  GraphReport,
  LocView,
  SymbolReport,
} from "@/lib/ipc";

/** The analysis a view is showing, and the state it is in. */
export type AnalysisFor<T> = Analysis<T>;

/** Subscribe to one analysis.
 *
 * The selector returns the whole `Analysis` object rather than its fields, because
 * Zustand compares the returned reference: destructuring into four primitives
 * would compare them individually and re-render on any one changing, which is the
 * same thing here but loses the identity of "this is one analysis state". */
export function useAnalysis<T>(id: ViewId): AnalysisFor<T> {
  return useClient((state) => state[id]) as AnalysisFor<T>;
}

/** Subscribe to the line count. */
export function useLoc(): Analysis<LocView> {
  return useAnalysis<LocView>("loc");
}

/** Subscribe to the declaration count. */
export function useSymbols(): Analysis<SymbolReport> {
  return useAnalysis<SymbolReport>("symbols");
}

/** Subscribe to the dependency graph. */
export function useGraph(): Analysis<GraphReport> {
  return useAnalysis<GraphReport>("graph");
}



/** The generation counter, for an effect that re-reads when the answer changes. */
export function useGeneration(): number {
  return useClient((state) => state.generation);
}
