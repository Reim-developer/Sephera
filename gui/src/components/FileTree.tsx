import {
  ChevronDown,
  ChevronRight,
  File,
  Folder,
  FolderOpen,
} from "lucide-react";

import { useFileTree } from "@/hooks/useFileTree";
import type { TreeNode } from "@/lib/ipc";
import styles from "@/styles/workbench.module.scss";

/**
 * The file tree in the sidebar.
 *
 * Rendering only. The loading, the expansion state and the in-flight guard all
 * live in `useFileTree`, because a component that fetches its own data has no
 * seam a test can substitute -- `scripts/check_gui_layers.py` enforces that, and
 * this file previously imported the registry directly.
 *
 * A node's children are rendered by `TreeBranch`, a component per level, so a
 * row re-renders without re-rendering everything under it. That is what keeps a
 * deep expansion of a repository with thousands of files from freezing the
 * sidebar, and it is why the recursion ends here rather than continuing through
 * the child rows.
 */
export function FileTree({
  root,
  selected,
  onOpen,
}: {
  root: string;
  selected: string | null;
  onOpen: (path: string) => void;
}) {
  const { levels, expanded, error, isPending, toggle } = useFileTree(root);

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
          pending={isPending}
          selected={selected}
          onToggle={toggle}
          onOpen={onOpen}
        />
      ))}
    </div>
  );
}

/** One row, with its branch below it.
 *
 * The row is a single node with no recursion of its own; the rows below it are
 * its children, each of which is a `TreeRow` of its own -- so the component tree
 * mirrors the expanded tree one level at a time, and React can stop at a row that
 * did not change. */
function TreeRow({
  node,
  depth,
  levels,
  expanded,
  pending,
  selected,
  onToggle,
  onOpen,
}: {
  node: TreeNode;
  depth: number;
  levels: Record<string, TreeNode[]>;
  expanded: ReadonlySet<string>;
  pending: (directory: string) => boolean;
  selected: string | null;
  onToggle: (path: string) => void;
  onOpen: (path: string) => void;
}) {
  const isOpen = expanded.has(node.path);
  const isSelected = selected === node.path;
  const children = levels[node.path];
  const isLoading = isOpen && children === undefined;

  const Directory = isOpen ? FolderOpen : Folder;

  return (
    <>
      <button
        type="button"
        role="treeitem"
        aria-selected={isSelected}
        aria-expanded={node.is_dir ? isOpen : undefined}
        aria-busy={isLoading || undefined}
        className={styles.tree__row}
        style={{ paddingLeft: 6 + depth * 12 }}
        onClick={() => (node.is_dir ? onToggle(node.path) : onOpen(node.path))}
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
          {node.is_dir ? <Directory size={14} /> : <File size={14} />}
        </span>
        <span className={styles.tree__label}>{name(node.path)}</span>
      </button>

      {node.is_dir && isOpen ? (
        <TreeBranch
          directory={node.path}
          depth={depth}
          levels={levels}
          expanded={expanded}
          pending={pending}
          selected={selected}
          onToggle={onToggle}
          onOpen={onOpen}
          loading={isLoading}
        />
      ) : null}
    </>
  );
}

/** The children of one open directory.
 *
 * Separate from the row so the subtree re-renders only when the subtree changes.
 * Rendering the children inline in `TreeRow` made every open-and-close re-render
 * every descendant, which is the stutter this component exists to remove.
 */
function TreeBranch({
  directory,
  depth,
  levels,
  expanded,
  pending,
  selected,
  onToggle,
  onOpen,
  loading,
}: {
  directory: string;
  depth: number;
  levels: Record<string, TreeNode[]>;
  expanded: ReadonlySet<string>;
  pending: (directory: string) => boolean;
  selected: string | null;
  onToggle: (path: string) => void;
  onOpen: (path: string) => void;
  loading: boolean;
}) {
  if (loading) {
    return (
      <div className={styles.tree__row} style={{ paddingLeft: 18 + depth * 12 }}>
        <span className={styles.tree__label}>…</span>
      </div>
    );
  }

  const children = levels[directory] ?? [];
  if (children.length === 0) {
    return (
      <div className={styles.tree__row} style={{ paddingLeft: 18 + depth * 12 }}>
        <span className={styles.tree__label} aria-hidden="true">
          ∅
        </span>
      </div>
    );
  }

  return (
    <>
      {children.map((child) => (
        <TreeRow
          key={child.path}
          node={child}
          depth={depth + 1}
          levels={levels}
          expanded={expanded}
          pending={pending}
          selected={selected}
          onToggle={onToggle}
          onOpen={onOpen}
        />
      ))}
    </>
  );
}

/** The final segment of a path. */
function name(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
