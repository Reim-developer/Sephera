/**
 * Layer 2: the store.
 *
 * One Zustand store, and the only place the client keeps state. A view reads it
 * and dispatches actions; it never holds a `useState` that outlives a render.
 *
 * The actions call services through the registry, so a view that says
 * `recompute()` has no idea whether the answer came from Tauri or from a test
 * double -- and that is the property the other three layers exist to provide.
 *
 * `epoch` is the number that makes cancellation work. A request carries one, and
 * when its reply arrives the store asks whether it is still the run anyone is
 * waiting for; if not, the reply is discarded rather than written over a newer
 * answer. That is what lets `Cancel` close the dialog at once, and it is also
 * what stops a slow reply for an old directory from replacing a fresh one.
 */

import { create, type StoreApi } from "zustand";

import type {
  FileDetail,
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
  file: Analysis<FileDetail>;
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
  /** Abandon whatever is running, and close the dialog. */
  cancel: () => void;
  /** Replace one level of the explorer's tree. */
  setTreeLevel: (directory: string, nodes: TreeNode[]) => void;
  /** Drop the tree, for a directory change. */
  clearTree: () => void;
}

/**
 * The epoch a new request carries, and the epoch below which nothing is wanted.
 *
 * Two counters rather than one, because they answer different questions:
 * `stamp` says which run this reply is for, `discarded` says whether anyone
 * still wants it. A cancel moves the second and leaves the first alone, which is
 * why a reply arriving after a cancel is dropped for being old rather than for
 * being the wrong run -- the run a cancel discards is usually the one you were
 * waiting for, and comparing stamps alone would let it through.
 */
let stamp = 0;
let discarded = 0;
function next_epoch(): number {
  stamp += 1;
  return stamp;
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

/**
 * Write an analysis's outcome, or drop it when it is stale.
 *
 * The epoch is compared on arrival rather than trusted: a reply for a run nobody
 * is waiting for is dropped, so a slow answer for an old directory cannot
 * replace a fresh one for a new one. The work still completed -- the host bumped
 * its own epoch -- so the reply came back carrying zero.
 */
function settle<T>(
  set: SetState,
  key: "loc" | "symbols" | "graph",
  reply: [T, number],
): void {
  const [data, epoch] = reply;
  // A reply at or below the discard mark is one nobody wants. `<=` rather than
  // `==` because a cancel outranks every run before it, not only the one that
  // was in flight when it happened.
  if (epoch <= discarded) return;
  set({ [key]: { data, error: null, busy: false } });
  bump(set);
}

/** Record an analysis's failure. */
function fail(
  set: SetState,
  key: "loc" | "symbols" | "graph",
  message: string,
): void {
  set({ [key]: { data: null, error: message, busy: false } });
  bump(set);
}

/** Run the line count, writing its result and its error into the store. */
async function runLoc(
  set: SetState,
  root: string,
  ignore: readonly string[],
  epoch: number,
): Promise<void> {
  set((state) => ({ loc: { ...state.loc, busy: true, error: null } }));
  try {
    settle(set, "loc", await services.loc.count(root, ignore, epoch));
  } catch (error) {
    fail(set, "loc", String(error));
  }
}

/** Run the declaration count. */
async function runSymbols(
  set: SetState,
  root: string,
  ignore: readonly string[],
  epoch: number,
): Promise<void> {
  set((state) => ({ symbols: { ...state.symbols, busy: true, error: null } }));
  try {
    settle(
      set,
      "symbols",
      await services.symbols.count(root, ignore, epoch),
    );
  } catch (error) {
    fail(set, "symbols", String(error));
  }
}

/** Run the reverse-dependency query. */
async function runGraph(
  set: SetState,
  root: string,
  target: string,
  ignore: readonly string[],
  epoch: number,
): Promise<void> {
  // No target means there is nothing to ask. `busy` is left alone so the view's
  // "pick a file" placeholder is what shows, rather than a spinner that never
  // resolves.
  if (!target) return;

  set((state) => ({ graph: { ...state.graph, busy: true, error: null } }));
  try {
    settle(
      set,
      "graph",
      await services.graph.dependents(root, target, null, ignore, epoch),
    );
  } catch (error) {
    fail(set, "graph", String(error));
  }
}

/** Count one file and list what it declares. */
async function runFile(
  set: SetState,
  root: string,
  path: string,
): Promise<void> {
  set((state) => ({ file: { ...state.file, busy: true, error: null } }));
  try {
    const data = await services.file.detail(root, path);
    set({ file: { data, error: null, busy: false } });
    bump(set);
  } catch (error) {
    set({ file: { data: null, error: String(error), busy: false } });
    bump(set);
  }
}

/** The store. */
export const useClient = create<ClientState>((set, get) => ({
  // Empty rather than `"."`. A relative default resolves against the host
  // process's working directory, which under `tauri dev` is `gui/src-tauri`
  // -- so `.` counted that directory, which is not the project the user
  // meant, and the count ran for a long time over a build tree.
  //
  // Empty means "no analysis yet", and the shell shows the picker rather
  // than a table of numbers about nowhere.
  root: "",
  ignore: [],
  target: "",
  selected: null,
  view: "loc",
  loc: never<LocView>(),
  symbols: never<SymbolReport>(),
  graph: never<GraphReport>(),
  file: never<FileDetail>(),
  tree: {},
  generation: 0,

  setRoot: (path) => {
    // An empty path clears the analysis rather than running it. The host would
    // resolve "" against its own working directory -- which under `tauri dev`
    // is `gui/src-tauri` -- so treating it as a directory counted somewhere
    // nobody chose.
    if (!path) {
      set({ root: "", loc: never<LocView>() });
      return;
    }
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
      void runFile(set, get().root, path);
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
    // One stamp per run, taken before the call so the reply can be compared
    // against it. The showing analysis is the one that runs.
    const stamp = next_epoch();
    if (view === "loc") void runLoc(set, root, ignore, stamp);
    else if (view === "symbols") void runSymbols(set, root, ignore, stamp);
    else void runGraph(set, root, target, ignore, stamp);
  },

  recomputeAll: () => {
    const { root, ignore, target } = get();
    // One stamp for the whole batch, so a cancel discards every reply in it at
    // once and a partial set of newer answers never appears beside an older one.
    const stamp = next_epoch();
    void runLoc(set, root, ignore, stamp);
    void runSymbols(set, root, ignore, stamp);
    if (target) void runGraph(set, root, target, ignore, stamp);
  },

  cancel: () => {
    // Move the discard mark past every run in flight, so each reply is dropped
    // as it arrives rather than racing the cancel. The host's own epoch is
    // bumped as well, and the work still completes; nobody waits for it.
    discarded = stamp;
    void services.dialog.cancel().then(() => {
      set((state) => ({
        loc: { ...state.loc, busy: false },
        symbols: { ...state.symbols, busy: false },
        graph: { ...state.graph, busy: false },
        file: { ...state.file, busy: false },
      }));
    });
  },

  setTreeLevel: (directory, nodes) =>
    set((state) => ({ tree: { ...state.tree, [directory]: nodes } })),

  clearTree: () => set({ tree: {} }),
}));
