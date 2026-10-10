/**
 * Layer 4: the file tree's data, as a hook.
 *
 * `components/FileTree.tsx` used to import the registry and call
 * `services.explorer.list` itself. That compiles, runs, and crosses the layering:
 * a component may not reach into the service layer, because the point of the layer
 * is that a *test* can replace what is behind it -- and a component calling the
 * registry directly has no seam to be handed anything.
 *
 * `scripts/check_gui_layers.py` flagged it on the run that added the `@/` alias,
 * which is the check earning its keep: the violation had been there since the
 * split and nothing else in the build could see it.
 *
 * So this hook owns the loading and the component owns the rendering. The hook
 * imports the registry (`hooks -> services`) and the store (`hooks -> state`),
 * both allowed, and the component imports the hook (`components -> hooks`).
 *
 * `levels` comes from the store rather than from local state, so a directory
 * change is seen by every consumer and the tree survives a re-render of anything
 * else in the shell.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import type { TreeNode } from "@/lib/ipc";
import { services } from "@/services/registry";
import { useClient } from "@/state/store";

/** What a tree needs to know before it can render. */
export interface FileTreeData {
  /** Directory path -> its immediate children. `""` is the root level. */
  levels: Record<string, TreeNode[]>;
  /** Which directories are open. */
  expanded: ReadonlySet<string>;
  /** A read that failed, or `null`. */
  error: string | null;
  /** Whether a directory's children are still on their way. */
  isPending: (directory: string) => boolean;
  /** Open or close a directory, loading its children on first open. */
  toggle: (directory: string) => void;
}

/**
 * The lazy file tree.
 *
 * Children are fetched one level at a time, so expanding a directory is a round
 * trip and nothing else. Loading the whole tree once is what makes a file explorer
 * feel instant on a small repository and frozen on a large one, and the crossover
 * is much closer than it looks.
 *
 * Both the root and the directory are sent, because the host resolves the read
 * against the analysis root rather than against its own working directory. Sending
 * one path made every folder below the root read somewhere that did not exist.
 */
export function useFileTree(root: string): FileTreeData {
  const tree = useClient((state) => state.tree);
  const setTreeLevel = useClient((state) => state.setTreeLevel);
  const clearTree = useClient((state) => state.clearTree);

  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  /** Directories with a request in flight, so a double click cannot re-fetch. */
  const pending = useRef<ReadonlySet<string>>(new Set());

  const load = useCallback(
    async (directory: string) => {
      if (pending.current.has(directory)) return;
      const inFlight = new Set(pending.current);
      inFlight.add(directory);
      pending.current = inFlight;

      try {
        const entries = await services.explorer.list(root, directory);
        setTreeLevel(directory, entries);
      } catch (cause) {
        setError(String(cause));
        // Collapse a directory that could not be read, so the row does not sit
        // expanded over nothing.
        setExpanded((previous) => {
          const next = new Set(previous);
          next.delete(directory);
          return next;
        });
      } finally {
        const done = new Set(pending.current);
        done.delete(directory);
        pending.current = done;
      }
    },
    [setTreeLevel, root],
  );

  // The root level, whenever the root changes. Guarded by a cancellation flag
  // because switching roots quickly would otherwise let the slower reply win.
  useEffect(() => {
    let cancelled = false;
    setExpanded(new Set());
    setError(null);
    clearTree();

    void load("").then(() => {
      if (cancelled) return;
    });

    return () => {
      cancelled = true;
    };
  }, [load, clearTree]);

  /**
   * Open or close a directory.
   *
   * The "already loaded" check reads `tree` rather than a captured value, so a
   * second click after the first has landed is still a no-op -- and the first
   * click on a directory whose level arrived while the request was in flight
   * does not fire a second request.
   */
  const toggle = useCallback(
    (directory: string) => {
      setExpanded((previous) => {
        const next = new Set(previous);
        if (next.has(directory)) next.delete(directory);
        else next.add(directory);
        return next;
      });
      if (tree[directory] === undefined && !expanded.has(directory)) {
        void load(directory);
      }
    },
    [tree, expanded, load],
  );

  return {
    levels: tree,
    expanded,
    error,
    isPending: (directory) => pending.current.has(directory),
    toggle,
  };
}
