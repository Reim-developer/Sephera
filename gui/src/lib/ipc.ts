/**
 * Layer 0: the transport and the vocabulary.
 *
 * This module owns `invoke` and every type that crosses the webview boundary.
 * It is the only file in the client that knows Tauri exists -- every other layer
 * speaks in the types below and never sees a `Promise` from the host.
 *
 * The rule that makes the layering real: a *service* imports this. Nothing else
 * does, for anything but a type. `scripts/check_gui_layers.py` enforces it,
 * because a rule nobody checks is a rule that survives one sprint.
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
export interface TreeNode {
  path: string;
  is_dir: boolean;
  children: TreeNode[];
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

/** A file in the dependency graph, with its degrees. */
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
  /** A path that looked local and could not be placed. */
  local_gap: boolean;
  /** Gated behind a `#[cfg(...)]`. */
  cfg_gated: boolean;
  kind: string;
}

/** The top-level counts a report carries. */
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

/** One declaration inside a single file. */
export interface FileDeclaration {
  name: string;
  /** `functions`, `types`, `enums`, or `constants`. */
  kind: string;
  /** 1-based line where the declaration's name appears. */
  line: number;
}

/** What the per-file panel shows. */
export interface FileDetail {
  path: string;
  code: number;
  comment: number;
  empty: number;
  size_bytes: number;
  /** `null` when the extension is not a bundled language. */
  language: string | null;
  declarations: FileDeclaration[];
}

/**
 * The command names the Rust host registers.
 *
 * Spelling one out twice is how a rename becomes a runtime failure that surfaces
 * as an empty table, so they are named once and the services import them.
 */
export const COMMANDS = {
  countLines: "count_lines",
  countDeclarations: "count_declarations",
  dependencyGraph: "dependency_graph",
  listTree: "list_tree",
  fileDetail: "file_detail",
  cancelCurrent: "cancel_current",
} as const;

/** Call a command on the Rust host.
 *
 * Re-exported from here rather than imported by each service so that a service
 * never imports Tauri directly -- which is what makes the whole service layer
 * substitutable in a test. */
export { invoke };
