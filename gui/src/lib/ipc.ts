/**
 * The IPC boundary, mirrored from the Rust side by hand.
 *
 * Every type here matches a struct in `crates/sephera_gui` or a `#[serde]`
 * derive in `crates/sephera_graph`. There is deliberately no code generator: the
 * shared vocabulary is small enough to read, and a generator would add a build
 * step that only re-runs when the Rust side changes -- which is exactly when a
 * stale type would be most expensive.
 *
 * The consequence is that a rename here and a rename in Rust can disagree. The
 * thing that catches it is `Error: field not found` at runtime, which is why
 * every command below returns a discriminated union rather than throwing: a
 * failure to decode is a value the caller can render, not a crash.
 */

import { invoke } from "@tauri-apps/api/core";

/** One row of the line-count table. */
export interface LanguageRow {
  language: string;
  files: number;
  code: number;
  comment: number;
  empty: number;
  size_bytes: number;
}

/** What `sephera loc --path .` produced. */
export interface LocView {
  base_path: string;
  rows: LanguageRow[];
  totals: LanguageRow;
  files_scanned: number;
  elapsed_ms: number;
  config_source: string | null;
}

/** One node of the sidebar's file tree. */
export interface TreeEntry {
  path: string;
  is_dir: boolean;
  children: TreeEntry[];
}

/** A declaration kind, as the Rust enum names it in snake case. */
export type SymbolKind = "functions" | "types" | "enums" | "constants";

/** Per-language declaration totals. */
export interface LanguageSymbols {
  language: string;
  files: number;
  counts: Partial<Record<SymbolKind, number>>;
}

/** What `sephera symbols --path .` produced. */
export interface SymbolReport {
  base_path: string;
  by_language: LanguageSymbols[];
  totals: Partial<Record<SymbolKind, number>>;
  files_scanned: number;
  files_skipped: number;
  languages_detected: number;
}

/** One node of the sidebar's file tree. */
export interface TreeNode {
  path: string;
  is_dir: boolean;
  children: TreeNode[];
}

/** A reverse-dependency query, as the graph command accepts it. */
export interface GraphReport {
  base_path: string;
  focus_paths: string[];
  depth: number | null;
  query: { DependsOn: string } | null;
  nodes: Array<{
    file_path: string;
    language: string;
    is_test: boolean;
  }>;
  edges: Array<{
    from: string;
    import_path: string;
    to: string | null;
    resolved: boolean;
    local_gap: boolean;
    kind: string;
  }>;
  metrics: Record<string, number>;
}

/** Count a directory. Extra patterns are merged after `.sephera.toml`'s. */
export async function countLines(
  path: string,
  ignore: string[],
): Promise<LocView> {
  return invoke<LocView>("count_lines", { path, ignore });
}

/** Count declarations per language, read from parse trees. */
export async function countDeclarations(
  path: string,
  ignore: string[],
): Promise<SymbolReport> {
  return invoke<SymbolReport>("count_declarations", { path, ignore });
}

/** Build a reverse-dependency graph: what breaks if `target` changes. */
export async function dependencyGraph(
  path: string,
  target: string,
  depth: number | null,
  ignore: string[],
): Promise<GraphReport> {
  return invoke<GraphReport>("dependency_graph", { path, target, depth, ignore });
}

/** List one level of a directory, for the file tree. */
export async function listTree(path: string): Promise<TreeEntry[]> {
  return invoke<TreeEntry[]>("list_tree", { path });
}
