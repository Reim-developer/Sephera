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
  FileDetail,
  GraphReport,
  LocView,
  SymbolReport,
  TreeNode,
} from "@/lib/ipc";

/** A reply that knows whether it is still the one being waited for. */
export type Stamped<T> = [T, number];

/** Line counting. */
export const locService = {
  async count(
    path: string,
    ignore: readonly string[],
    epoch: number,
  ): Promise<Stamped<LocView>> {
    return invoke<Stamped<LocView>>(COMMANDS.countLines, {
      path,
      ignore: [...ignore],
      epoch,
    });
  },
};

/** Declaration counting, read from parse trees. */
export const symbolsService = {
  async count(
    path: string,
    ignore: readonly string[],
    epoch: number,
  ): Promise<Stamped<SymbolReport>> {
    return invoke<Stamped<SymbolReport>>(COMMANDS.countDeclarations, {
      path,
      ignore: [...ignore],
      epoch,
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
    epoch = 0,
  ): Promise<Stamped<GraphReport>> {
    return invoke<Stamped<GraphReport>>(COMMANDS.dependencyGraph, {
      path,
      target,
      depth,
      ignore: [...ignore],
      epoch,
    });
  },
};

/** The file tree in the sidebar.
 *
 * Two arguments, because the host needs to know where the analysis starts and
 * which directory inside it to list. An earlier version sent one path and had the
 * host resolve it against its own working directory -- so every folder below the
 * root read a path that did not exist, the read failed, and the folder collapsed
 * with nothing said about why.
 *
 * `directory` is empty for the root level and a path relative to the root below
 * that, which is exactly what the host returns as a node's path. */
export const explorerService = {
  async list(root: string, directory: string): Promise<TreeNode[]> {
    return invoke<TreeNode[]>(COMMANDS.listTree, { root, directory });
  },
};

/** One file's counts, for the per-file panel.
 *
 * `sephera loc --path <file>` refuses a file outright, so this is not the
 * directory scan filtered: it is the same two primitives the scanner uses, which
 * is what makes a file's numbers agree with the table above it. */
export const fileService = {
  async detail(root: string, path: string): Promise<FileDetail> {
    return invoke<FileDetail>(COMMANDS.fileDetail, { root, path });
  },
};

/** Whatever is running is no longer the run anyone is waiting for. */
export const dialogService = {
  async cancel(): Promise<void> {
    await invoke(COMMANDS.cancelCurrent);
  },
};

export { pickDirectory } from "./dialog";
