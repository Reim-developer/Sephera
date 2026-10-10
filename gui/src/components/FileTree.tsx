import { useCallback, useEffect, useRef, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  File,
  Folder,
  FolderOpen,
} from "lucide-react";

import { listTree, type TreeNode } from "../lib/ipc";
import styles from "../styles/workbench.module.scss";

/** How deep to expand before insisting the user ask for more.
 *
 * Two levels is enough to see `crates/` and `sephera_core/` in a workspace of
 * crates without walking every leaf of a tree that may be tens of thousands of
 * files. Expanding nothing would make the first render the whole repository,
 * which is what the lazy load exists to avoid. */
const AUTO_EXPAND_DEPTH = 2;

/**
 * The file tree in the sidebar.
 *
 * Children are fetched one level at a time, so expanding a directory is a round
 * trip and nothing else. Loading the whole tree once is what makes a file
 * explorer feel instant on a small repository and frozen on a large one, and the
 * crossover is much closer than it looks.
 */
export function FileTree({
  root,
  onOpen,
  selected,
}: {
  root: string;
  onOpen: (path: string) => void;
  selected: string | null;
}) {
  /** Directory path -> its immediate children. `""` is the root level. */
  const [levels, setLevels] = useState<Record<string, TreeNode[]>>({});
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  /** Directories with a request in flight, so a double click cannot re-fetch. */
  const pending = useRef<ReadonlySet<string>>(new Set());

  const load = useCallback(async (directory: string) => {
    if (pending.current.has(directory)) return;
    const inFlight = new Set(pending.current);
    inFlight.add(directory);
    pending.current = inFlight;

    try {
      const entries = await listTree(directory === "" ? root : directory);
      setLevels((previous) =>
        previous[directory] ? previous : { ...previous, [directory]: entries },
      );
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
  }, [root]);

  // The root level, whenever the root changes. Guarded by a cancellation flag
  // because switching roots quickly would otherwise let the slower reply win.
  useEffect(() => {
    let cancelled = false;
    setLevels({});
    setExpanded(new Set());
    setError(null);

    void load("").then(() => {
      if (cancelled) return;
    });

    return () => {
      cancelled = true;
    };
  }, [load]);

  function toggle(directory: string) {
    setExpanded((previous) => {
      const next = new Set(previous);
      if (next.has(directory)) next.delete(directory);
      else next.add(directory);
      return next;
    });
    const already = levels[directory] !== undefined || expanded.has(directory);
    if (!already) void load(directory);
  }

  const nodes = levels[""] ?? [];

  if (error && nodes.length === 0) {
    return (
      <p className={styles.sidebar__empty}>Cannot read the tree: {error}</p>
    );
  }
  if (nodes.length === 0) {
    return <p className={styles.sidebar__empty}>No files to show.</p>;
  }

  return (
    <div className={styles.tree} role="tree" aria-label="Files">
      {nodes.map((node) => (
        <TreeRow
          key={node.path}
          node={node}
          depth={0}
          levels={levels}
          expanded={expanded}
          selected={selected}
          onToggle={toggle}
          onOpen={onOpen}
        />
      ))}
    </div>
  );
}

function TreeRow({
  node,
  depth,
  levels,
  expanded,
  selected,
  onToggle,
  onOpen,
}: {
  node: TreeNode;
  depth: number;
  levels: Record<string, TreeNode[]>;
  expanded: ReadonlySet<string>;
  selected: string | null;
  onToggle: (path: string) => void;
  onOpen: (path: string) => void;
}) {
  const isOpen = expanded.has(node.path);
  const isSelected = selected === node.path;
  const children = levels[node.path];

  // A directory expands on the same click that a file opens with. A separate
  // disclosure triangle is one more thing to aim at.
  function activate() {
    if (node.is_dir) onToggle(node.path);
    else onOpen(node.path);
  }

  const Directory = isOpen ? FolderOpen : Folder;

  return (
    <>
      <button
        type="button"
        role="treeitem"
        aria-selected={isSelected}
        aria-expanded={node.is_dir ? isOpen : undefined}
        className={styles.tree__row}
        style={{ paddingLeft: 6 + depth * 12 }}
        onClick={activate}
        title={node.path}
      >
        <span className={styles.tree__chevron} aria-hidden="true">
          {node.is_dir ? (
            isOpen ? (
              <ChevronDown size={12} />
            ) : (
              <ChevronRight size={12} />
            )
          ) : null}
        </span>
        <span className={styles.tree__icon} aria-hidden="true">
          {node.is_dir ? (
            <Directory size={14} />
          ) : (
            <File size={14} />
          )}
        </span>
        <span className={styles.tree__label}>{name(node.path)}</span>
      </button>

      {node.is_dir && isOpen ? (
        children === undefined ? (
          // Not loaded yet: a placeholder row, so expanding does not leave an
          // empty gap where the contents are about to appear.
          <div
            className={styles.tree__row}
            style={{ paddingLeft: 18 + depth * 12 }}
          >
            <span className={styles.tree__label}>…</span>
          </div>
        ) : children.length === 0 ? (
          <div
            className={styles.tree__row}
            style={{ paddingLeft: 18 + depth * 12 }}
          >
            <span className={styles.tree__label} aria-hidden="true">
              ∅
            </span>
          </div>
        ) : depth >= AUTO_EXPAND_DEPTH ? null : (
          children.map((child) => (
            <TreeRow
              key={child.path}
              node={child}
              depth={depth + 1}
              levels={levels}
              expanded={expanded}
              selected={selected}
              onToggle={onToggle}
              onOpen={onOpen}
            />
          ))
        )
      ) : null}

      {/* Past the automatic depth the branch is rendered but its children are
          fetched only when opened -- which is the point of the depth limit. The
          rows are still interactive and their own expansion loads the next
          level on demand. */}
    </>
  );
}

/** The final segment of a path. */
function name(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
