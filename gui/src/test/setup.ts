/**
 * Test setup: the service registry is replaced once, and every caller follows.
 *
 * This is what the layering is for. A store test never touches Tauri -- it hands
 * the registry a service that resolves to a fixture -- and a component test
 * renders a view against a populated store without a host. Both are the same
 * substitution at one place, which is the only reason the layers are worth
 * having.
 */

import { beforeEach, vi } from "vitest";
import { withServices } from "@/services/registry";
import type { ServiceRegistry } from "@/services/registry";
import type {
  GraphReport,
  LocView,
  SymbolReport,
  TreeNode,
} from "@/lib/ipc";

/** A line count that says nothing was found. */
export function emptyLoc(overrides: Partial<LocView> = {}): LocView {
  return {
    base_path: "/project",
    rows: [],
    totals: {
      language: "Totals",
      files: 0,
      code: 0,
      comment: 0,
      empty: 0,
      size_bytes: 0,
    },
    files_scanned: 0,
    elapsed_ms: 5,
    config_source: null,
    ...overrides,
  };
}

/** A line count over two languages, as `sephera loc` would report them. */
export function twoLanguageLoc(): LocView {
  const rust = { language: "Rust", files: 2, code: 20, comment: 3, empty: 4, size_bytes: 220 };
  const toml = { language: "TOML", files: 1, code: 4, comment: 0, empty: 1, size_bytes: 44 };
  return emptyLoc({
    rows: [rust, toml],
    totals: {
      language: "Totals",
      files: 3,
      code: 24,
      comment: 3,
      empty: 5,
      size_bytes: 264,
    },
    files_scanned: 3,
    config_source: "/project/.sephera.toml",
  });
}

/** A declaration count over one language. */
export function declarations(): SymbolReport {
  return {
    base_path: "/project",
    by_language: [
      {
        language: "Rust",
        files: 2,
        counts: { functions: 20, types: 4, enums: 1, constants: 2 },
      },
    ],
    totals: { functions: 20, types: 4, enums: 1, constants: 2 },
    files_scanned: 2,
    files_skipped: 0,
    languages_detected: 1,
  };
}

/** A reverse-dependency report with two dependents. */
export function dependents(): GraphReport {
  return {
    base_path: "/project",
    focus_paths: [],
    depth: null,
    query: { DependsOn: "a.rs" },
    nodes: [
      {
        file_path: "b.rs",
        language: "Rust",
        imports_count: 12,
        imported_by_count: 1,
      },
      {
        file_path: "c.rs",
        language: "Rust",
        imports_count: 1,
        imported_by_count: 0,
      },
    ],
    edges: [],
    metrics: {
      total_files: 3,
      total_internal_edges: 2,
      self_references: 0,
      total_external_edges: 5,
      unresolved_local_edges: 1,
      cfg_gated_edges: 0,
      circular_dependencies: 0,
    },
  };
}

/** One level of a tree. */
export function treeLevel(): TreeNode[] {
  return [
    { path: "src", is_dir: true, children: [] },
    { path: "a.rs", is_dir: false, children: [] },
  ];
}

/** The registry a test installs.
 *
 * Every method resolves to a fixture, and every method records that it was called
 * -- which is how a test asserts the *store* asked the *service*, rather than
 * asserting on a rendered number that could have come from anywhere.
 */
export function installServices(): Record<
  string,
  ReturnType<typeof vi.fn>
> {
  const loc = vi.fn().mockResolvedValue(twoLanguageLoc());
  const symbols = vi.fn().mockResolvedValue(declarations());
  const graph = vi.fn().mockResolvedValue(dependents());
  const explorer = vi.fn().mockResolvedValue(treeLevel());

  const calls = {
    loc,
    symbols,
    graph,
    explorer,
  };

  withServices({
    loc: { count: loc } as unknown as ServiceRegistry["loc"],
    symbols: { count: symbols } as unknown as ServiceRegistry["symbols"],
    graph: {
      dependents: graph,
    } as unknown as ServiceRegistry["graph"],
    explorer: { list: explorer } as unknown as ServiceRegistry["explorer"],
  });

  return calls;
}

/** Restore the live registry before each test.
 *
 * A registry that survives a test is a registry that hands the next test a
 * fixture from the last one, which is the most expensive kind of flake to debug.
 */
beforeEach(() => {
  withServices({});
});
