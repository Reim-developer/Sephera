/**
 * Layer 2: the store.
 *
 * One Zustand store, and the only place the client keeps state. A view reads it
 * and dispatches actions; it never holds a `useState` that outlives a render.
 *
 * The actions call services through the registry, so a view that says
 * `recompute()` has no idea whether the answer came from Tauri or from a test
 * double -- and that is the property the other three layers exist to provide.
 */

import { create, type StoreApi } from "zustand";

import type {
  GraphReport,
  LocView,
  SymbolReport,
  TreeNode,
} from "@/lib/ipc";
import { services } from "@/services/registry";

/** Which analysis the client is showing. */
export type ViewId = "loc" | "symbols" | "graph";

/** One analysis, and the state it is in. */
export interface Analysis<T> {
  data: T | null;
  error: string | null;
  busy: boolean;
}

/** The store's own `setState`, so a helper can write without being a closure.
 *
 * Typed as `StoreApi<...>['setState']` rather than as `(partial) => void`,
 * because the real setter is overloaded -- it takes a partial *or* a function of
 * the previous state -- and a helper that declares the narrower signature both
 * loses that and breaks the inference of every selector on the hook above. */
type SetState = StoreApi<ClientState>["setState"];

/** Everything the client knows. */
export interface ClientState {
  // ---- inputs ------------------------------------------------------------
  /** The directory being analysed. */
  root: string;
  /** Extra ignore patterns, merged after `.sephera.toml`'s. */
  ignore: readonly string[];
  /** The file the graph measures from. */
  target: string;
  /** The file selected in the explorer. */
  selected: string | null;
  /** Which analysis is showing. */
  view: ViewId;

  // ---- results -----------------------------------------------------------
  loc: Analysis<LocView>;
  symbols: Analysis<SymbolReport>;
  graph: Analysis<GraphReport>;
  /** The explorer's tree, keyed by directory to keep the levels apart. */
  tree: Record<string, TreeNode[]>;

  /** A counter, so "again" is a value rather than a re-created callback. */
  generation: number;

  // ---- actions -----------------------------------------------------------
  setRoot: (path: string) => void;
  setIgnore: (patterns: readonly string[]) => void;
  setTarget: (path: string) => void;
  setSelected: (path: string | null) => void;
  setView: (view: ViewId) => void;
  /** Run the showing analysis again. */
  recompute: () => void;
  /** Run every analysis, for a directory or pattern change. */
  recomputeAll: () => void;
  /** Replace one level of the explorer's tree. */
  setTreeLevel: (directory: string, nodes: TreeNode[]) => void;
  /** Drop the tree, for a directory change. */
  clearTree: () => void;
}

/** An analysis that has never run. */
function never<T>(): Analysis<T> {
  return { data: null, error: null, busy: false };
}

/**
 * Bump the generation, so views know the answer changed.
 *
 * A view subscribes to this even when it does not use it, because a recompute
 * that returned the same object identity would not otherwise re-render and the
 * view would show a stale answer after an edit that changed the number.
 */
function bump(set: SetState): void {
  set((state) => ({ generation: state.generation + 1 }));
}

/** Run the line count, writing its result and its error into the store. */
async function runLoc(
  set: SetState,
  root: string,
  ignore: readonly string[],
): Promise<void> {
  set((state) => ({ loc: { ...state.loc, busy: true, error: null } }));
  try {
    const data = await services.loc.count(root, ignore);
    set({ loc: { data, error: null, busy: false } });
    bump(set);
  } catch (error) {
    set({ loc: { data: null, error: String(error), busy: false } });
    bump(set);
  }
}

/** Run the declaration count. */
async function runSymbols(
  set: SetState,
  root: string,
  ignore: readonly string[],
): Promise<void> {
  set((state) => ({ symbols: { ...state.symbols, busy: true, error: null } }));
  try {
    const data = await services.symbols.count(root, ignore);
    set({ symbols: { data, error: null, busy: false } });
    bump(set);
  } catch (error) {
    set({ symbols: { data: null, error: String(error), busy: false } });
    bump(set);
  }
}

/** Run the reverse-dependency query. */
async function runGraph(
  set: SetState,
  root: string,
  target: string,
  ignore: readonly string[],
): Promise<void> {
  // No target means there is nothing to ask. `busy` is left alone so the view's
  // "pick a file" placeholder is what shows, rather than a spinner that never
  // resolves.
  if (!target) return;

  set((state) => ({ graph: { ...state.graph, busy: true, error: null } }));
  try {
    const data = await services.graph.dependents(root, target, null, ignore);
    set({ graph: { data, error: null, busy: false } });
    bump(set);
  } catch (error) {
    set({ graph: { data: null, error: String(error), busy: false } });
    bump(set);
  }
}

/** The store. */
export const useClient = create<ClientState>((set, get) => ({
  root: ".",
  ignore: [],
  target: "",
  selected: null,
  view: "loc",
  loc: never<LocView>(),
  symbols: never<SymbolReport>(),
  graph: never<GraphReport>(),
  tree: {},
  generation: 0,

  setRoot: (path) => {
    set({ root: path });
    // A new directory invalidates every analysis, and the tree with it. Running
    // only the showing one would leave the others stale and one click away from
    // being shown as though they were current.
    get().clearTree();
    get().recomputeAll();
  },

  setIgnore: (patterns) => {
    set({ ignore: patterns });
    get().recomputeAll();
  },

  setTarget: (path) => {
    // Choosing a target is also choosing to look at the graph: the question asked
    // for the file's radius, which is the graph's answer.
    set({ target: path, view: "graph" });
    get().recompute();
  },

  setSelected: (path) => {
    set({ selected: path });
    // A file picked in the explorer is also the graph's target, so the view has
    // something to show the moment a file is chosen.
    if (path) {
      set({ target: path });
      get().recompute();
    }
  },

  setView: (view) => {
    set({ view });
    // Switching to a view that has never run would otherwise show an empty
    // placeholder that looks like a result.
    get().recompute();
  },

  recompute: () => {
    const { view, root, ignore, target } = get();
    if (view === "loc") void runLoc(set, root, ignore);
    else if (view === "symbols") void runSymbols(set, root, ignore);
    else void runGraph(set, root, target, ignore);
  },

  recomputeAll: () => {
    const { root, ignore, target } = get();
    void runLoc(set, root, ignore);
    void runSymbols(set, root, ignore);
    if (target) void runGraph(set, root, target, ignore);
  },

  setTreeLevel: (directory, nodes) =>
    set((state) => ({ tree: { ...state.tree, [directory]: nodes } })),

  clearTree: () => set({ tree: {} }),
}));
