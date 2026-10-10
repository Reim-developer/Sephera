import { AlertTriangle, FileCode2 } from "lucide-react";

import { ProgressBar } from "@/components/ProgressBar";
import { useGeneration, useGraph } from "@/hooks/useAnalysis";
import { run } from "@/platform/commands";
import { useClient } from "@/state/store";
import type { GraphNode } from "@/lib/ipc";
import { QueryRow } from "./QueryRow";
import styles from "@/styles/views.module.scss";

/**
 * The reverse-dependency view: what breaks if this file changes.
 *
 * This is the one view where a graphical client genuinely beats a terminal, and
 * the reason is in `DependentList`: every row is a button that makes that row the
 * new target, so a reader walks the blast radius outward instead of re-running a
 * command per hop.
 *
 * It holds no state. The walk, the query text and the results are all the
 * store's, which is what lets the same walk be driven from the explorer.
 */
export function DependenciesView() {
  const { data, error, busy } = useGraph();
  const target = useClient((state) => state.target);
  const generation = useGeneration();
  void generation;

  // Dependents, sorted by how much they import: a file that reaches the target
  // through twelve paths is a more interesting neighbour than one that reaches it
  // through one.
  const dependents: readonly GraphNode[] = data
    ? data.nodes
        .filter((node) => node.file_path !== target)
        .sort((left, right) => right.imports_count - left.imports_count)
    : [];

  if (!target) {
    return (
      <div className={styles.placeholder}>
        <span className={styles.placeholder__title}>Pick a file</span>
        <span className={styles.placeholder__hint}>
          Choose one in the explorer, or type a path and press{" "}
          <kbd className={styles.kbd}>Enter</kbd>. The graph is built for that
          file, and everything is rebuilt when the file changes.
        </span>
      </div>
    );
  }

  return (
    <div className={styles.view}>
      <header className={styles.view__header}>
        <h2 className={styles.view__title}>Dependencies</h2>
        <p className={styles.view__subtitle}>
          {data
            ? `${dependents.length} file${dependents.length === 1 ? "" : "s"} depend on this`
            : "building the graph"}
        </p>
      </header>

      {/* The query row belongs to this view, because typing a path is how this
          analysis is addressed -- not the shell's, whose directory field applies
          to all three alike. */}
      <QueryRow />

      {error ? (
        <div className={styles.banner}>
          <span className={styles.banner__icon} aria-hidden="true">
            <AlertTriangle size={16} />
          </span>
          <span>
            {error}
            <br />
            <span className={styles.banner__hint}>
              The path has to exist inside the directory being analysed. A path
              outside it, or one the resolver never placed, has no graph to show.
            </span>
          </span>
        </div>
      ) : busy && !data ? (
        <>
          <ProgressBar label="Building the dependency graph" />
          <p className={styles.view__subtitle}>
            The whole tree is parsed once, then traversed in reverse. On a
            workspace this size it is well under a second; on a large repository
            it is a few seconds.
          </p>
        </>
      ) : data ? (
        <>
          <Summary />
          <DependentList
            dependents={dependents}
            onSelect={(path) => void run("sephera.setTarget", { path })}
          />
        </>
      ) : null}
    </div>
  );
}

/** The metrics that say whether the answer above can be trusted. */
function Summary() {
  const metrics = useClient((state) => state.graph.data?.metrics ?? null);
  if (!metrics) return null;

  return (
    <dl className={styles.summary}>
      <div>
        <dt>Files</dt>
        <dd>{metrics.total_files.toLocaleString()}</dd>
      </div>
      <div>
        <dt>Internal edges</dt>
        <dd>{metrics.total_internal_edges.toLocaleString()}</dd>
      </div>
      {/* A number that qualifies the answer is coloured, because a reader
          scanning six figures should not have to remember which one was the
          caveat. */}
      <div>
        <dt>Unresolved local</dt>
        <dd
          className={
            metrics.unresolved_local_edges > 0 ? styles.summary__warn : undefined
          }
        >
          {metrics.unresolved_local_edges.toLocaleString()}
        </dd>
      </div>
      <div>
        <dt>Cfg-gated</dt>
        <dd>{metrics.cfg_gated_edges.toLocaleString()}</dd>
      </div>
    </dl>
  );
}

/** The dependents, each clickable to become the next target. */
function DependentList({
  dependents,
  onSelect,
}: {
  dependents: readonly GraphNode[];
  onSelect: (path: string) => void;
}) {
  if (dependents.length === 0) {
    return (
      <p className={styles.table__empty}>Nothing in this tree imports the target.</p>
    );
  }

  return (
    <table className={styles.table}>
      <caption className="sr-only">Files that depend on the target</caption>
      <thead>
        <tr>
          <th scope="col">File</th>
          <th scope="col">Language</th>
          <th scope="col" className="numeric">
            Imports
          </th>
        </tr>
      </thead>
      <tbody>
        {dependents.map((node) => (
          <tr key={node.file_path}>
            {/* The whole row is a button, so the click target is the file name
                rather than a small chevron beside it. */}
            <td>
              <button
                type="button"
                className={styles.link}
                onClick={() => onSelect(node.file_path)}
                title={`Trace what depends on ${node.file_path}`}
              >
                <FileCode2 size={13} aria-hidden="true" /> {node.file_path}
              </button>
            </td>
            <td>{node.language ?? "—"}</td>
            <td className="numeric">{node.imports_count.toLocaleString()}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
