
/**
 * Layer 1: the services.
 *
 * One function per host capability, each importing nothing but `lib/ipc`. No
 * state, no React, no knowledge of which view is asking. That is the whole point:
 * a service is the seam where a test substitutes the host, and it works as a seam
 * only because a service has nothing else in it.
 *
 * A view never imports a service. It imports the store, the store calls the
 * registry, and the registry is what a test replaces.
 */

import { COMMANDS, invoke } from "@/lib/ipc";
import type {
  GraphReport,
  LocView,
  SymbolReport,
  TreeNode,
} from "@/lib/ipc";

/** Line counting. */
export const locService = {
  async count(path: string, ignore: readonly string[]): Promise<LocView> {
    return invoke<LocView>(COMMANDS.countLines, { path, ignore: [...ignore] });
  },
};

/** Declaration counting, read from parse trees. */
export const symbolsService = {
  async count(path: string, ignore: readonly string[]): Promise<SymbolReport> {
    return invoke<SymbolReport>(COMMANDS.countDeclarations, {
      path,
      ignore: [...ignore],
    });
  },
};

/**
 * Reverse dependency: what breaks if `target` changes.
 *
 * `depth` is accepted and always passed as `null` for now -- the CLI has a
 * `--depth` and the command takes one, but no view exposes it. Keeping the
 * parameter is what stops adding the control later from being a signature change
 * through three files.
 */
export const graphService = {
  async dependents(
    path: string,
    target: string,
    depth: number | null = null,
    ignore: readonly string[] = [],
  ): Promise<GraphReport> {
    return invoke<GraphReport>(COMMANDS.dependencyGraph, {
      path,
      target,
      depth,
      ignore: [...ignore],
    });
  },
};

/** The file tree in the sidebar. */
export const explorerService = {
  async list(path: string): Promise<TreeNode[]> {
    return invoke<TreeNode[]>(COMMANDS.listTree, { path });
  },
};

export { pickDirectory } from "@/services/dialog";
