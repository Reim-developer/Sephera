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
  const { levels, expanded, error, isPending, toggle, autoExpandDepth } =
    useFileTree(root);

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
          autoExpandDepth={autoExpandDepth}
        />
      ))}
    </div>
  );
}

/** One row, and its subtree. */
function TreeRow({
  node,
  depth,
  levels,
  expanded,
  pending,
  selected,
  onToggle,
  onOpen,
  autoExpandDepth,
}: {
  node: TreeNode;
  depth: number;
  levels: Record<string, TreeNode[]>;
  expanded: ReadonlySet<string>;
  pending: (directory: string) => boolean;
  selected: string | null;
  onToggle: (path: string) => void;
  onOpen: (path: string) => void;
  autoExpandDepth: number;
}) {
  const isOpen = expanded.has(node.path);
  const isSelected = selected === node.path;
  const children = levels[node.path];
  const isLoading = isOpen && children === undefined;

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
        aria-busy={isLoading || undefined}
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
          {node.is_dir ? <Directory size={14} /> : <File size={14} />}
        </span>
        <span className={styles.tree__label}>{name(node.path)}</span>
      </button>

      {node.is_dir && isOpen ? (
        isLoading ? (
          // A placeholder row, so expanding does not leave an empty gap where the
          // contents are about to appear. `aria-live` is absent on purpose: the
          // rows themselves announce the change.
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
        ) : depth >= autoExpandDepth ? null : (
          children.map((child) => (
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
              autoExpandDepth={autoExpandDepth}
            />
          ))
        )
      ) : null}

      {/* Past the automatic depth the branch is rendered but its children are
          fetched only when opened -- which is the point of the depth limit. The
          rows are still interactive and their own expansion loads the next level
          on demand. */}
    </>
  );
}

/** The final segment of a path. */
function name(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
