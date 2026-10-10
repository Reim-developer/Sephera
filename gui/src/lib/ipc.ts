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

/** A file in the graph, with its degrees. */
export interface GraphNode {
  file_path: string;
  /** `null` when the language could not be detected. */
  language: string | null;
  /** How many imports this file makes. */
  imports_count: number;
  /** How many files import this file -- the number `impact` reports. */
  imported_by_count: number;
}

/** One import statement, resolved or not. */
export interface GraphEdge {
  from: string;
  import_path: string;
  /** `null` for an external dependency or a path that could not be placed. */
  to: string | null;
  resolved: boolean;
  /**
   * True when the path looked local and could not be placed. Distinct from
   * `!resolved`, which also covers every external crate: a gap is a resolver
   * defect, an external import is not.
   */
  local_gap: boolean;
  /** Gated behind a `#[cfg(...)]`. */
  cfg_gated: boolean;
  kind: string;
}

/** A top-level count, as the report's metrics. */
export interface GraphMetrics {
  total_files: number;
  total_internal_edges: number;
  self_references: number;
  total_external_edges: number;
  unresolved_local_edges: number;
  cfg_gated_edges: number;
  circular_dependencies: number;
}

/**
 * A reverse-dependency report: everything that depends on one file.
 *
 * This is the view where a graphical client earns its keep. The CLI can print
 * the number and stop there; it cannot let a reader click a dependent and then
 * ask the same question about *that* file, which is how a blast radius is
 * actually explored.
 */
export interface GraphReport {
  base_path: string;
  focus_paths: string[];
  depth: number | null;
  query: { DependsOn: string } | null;
  nodes: GraphNode[];
  edges: GraphEdge[];
  metrics: GraphMetrics;
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
