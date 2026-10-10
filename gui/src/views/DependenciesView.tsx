import { useCallback, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  Crosshair,
  FileCode2,
  Loader,
} from "lucide-react";

import { dependencyGraph, type GraphNode, type GraphReport } from "../lib/ipc";
import styles from "../styles/views.module.scss";

/**
 * The reverse-dependency view: what breaks if this file changes.
 *
 * This is the one view where a graphical client genuinely beats a terminal, and
 * the reason is in the last two lines of the component. `sephera impact` answers
 * for one file and stops. Here, every row is a button that makes *that* file the
 * target, so a reader walks the blast radius outward instead of re-running a
 * command per hop.
 *
 * The target's own `imported_by_count` is not what is shown. It is the count over
 * the whole tree, which includes paths the reverse traversal never reaches -- so
 * showing it would claim a radius larger than the one reported. The list length
 * is the honest number, and the header says so.
 */
export function DependenciesView({
  path,
  target,
  onTargetChange,
  reloadToken,
}: {
  path: string;
  /** The file to measure from, chosen in the sidebar or typed here. */
  target: string;
  onTargetChange: (path: string) => void;
  reloadToken: number;
}) {
  const [query, setQuery] = useState(target);
  const [report, setReport] = useState<GraphReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  /** The files the query returned, in the order they should be walked. */
  const [trail, setTrail] = useState<string[]>([]);

  const run = useCallback(
    async (file: string) => {
      setBusy(true);
      setError(null);
      try {
        setReport(await dependencyGraph(path, file, null, []));
        setTrail((previous) =>
          previous[previous.length - 1] === file ? previous : [...previous, file],
        );
      } catch (cause) {
        setError(String(cause));
        setReport(null);
      } finally {
        setBusy(false);
      }
    },
    [path],
  );

  // A new target from outside -- the sidebar's file tree, say -- replaces the
  // trail rather than extending it. The trail is a walk backwards through
  // dependents, and jumping somewhere else is a different walk.
  useEffect(() => {
    setQuery(target);
    void run(target);
    setTrail([target]);
  }, [run, target, reloadToken]);

  // Dependents, sorted by how much they import: a file that reaches the target
  // through twelve paths is a more interesting neighbour than one that reaches
  // it through one.
  const dependents = useMemo(() => {
    if (!report) return [];
    return report.nodes
      .filter((node) => node.file_path !== target)
      .sort((left, right) => right.imports_count - left.imports_count);
  }, [report, target]);

  if (!target) {
    return (
      <div className={styles.placeholder}>
        <span className={styles.placeholder__title}>Pick a file</span>
        <span className={styles.placeholder__hint}>
          Choose one in the explorer, or type a path and press{" "}
          <kbd className={styles.kbd}>Enter</kbd>. The graph is built for that
          file and everything is rebuilt when the file changes.
        </span>
      </div>
    );
  }

  return (
    <div className={styles.view}>
      <header className={styles.view__header}>
        <h2 className={styles.view__title}>Dependencies</h2>
        <p className={styles.view__subtitle}>
          {report
            ? `${dependents.length} file${dependents.length === 1 ? "" : "s"} depend on this`
            : "building the graph"}
        </p>
      </header>

      <div className={styles.query}>
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          aria-label="File to trace"
          placeholder="crates/sephera_graph/src/resolver.rs"
          onKeyDown={(event) => {
            if (event.key === "Enter" && query.trim()) {
              onTargetChange(query.trim());
            }
          }}
        />
        <button
          type="button"
          onClick={() => query.trim() && onTargetChange(query.trim())}
        >
          <Crosshair size={13} aria-hidden="true" /> Trace
        </button>
        {trail.length > 1 ? (
          <button
            type="button"
            onClick={() => {
              const previous = trail[trail.length - 2];
              setTrail((walk) => walk.slice(0, -1));
              onTargetChange(previous);
            }}
          >
            <ArrowLeft size={13} aria-hidden="true" /> Back
          </button>
        ) : null}
      </div>

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
      ) : busy && !report ? (
        <div className={styles.placeholder}>
          <span className={styles.placeholder__title}>
            <Loader size={14} aria-hidden="true" /> Building the graph
          </span>
          <span className={styles.placeholder__hint}>
            The whole tree is parsed once, then traversed in reverse. On a
            workspace this size it is well under a second; on a large repository
            it is a few seconds.
          </span>
        </div>
      ) : report ? (
        <>
          <WalkSummary report={report} trail={trail} />
          <DependentList
            dependents={dependents}
            target={target}
            onSelect={onTargetChange}
          />
        </>
      ) : null}
    </div>
  );
}

/** The metrics that say whether the answer above can be trusted. */
function WalkSummary({
  report,
  trail,
}: {
  report: GraphReport;
  trail: readonly string[];
}) {
  const { metrics } = report;

  return (
    <dl className={styles.summary}>
      <div>
        <dt>Walk</dt>
        <dd>
          {trail.map((file, index) => (
            <span key={file}>
              {index > 0 ? " → " : ""}
              <code>{name(file)}</code>
            </span>
          ))}
        </dd>
      </div>
      <div>
        <dt>Files</dt>
        <dd>{metrics.total_files.toLocaleString()}</dd>
      </div>
      <div>
        <dt>Internal edges</dt>
        <dd>{metrics.total_internal_edges.toLocaleString()}</dd>
      </div>
      {/* Two numbers that qualify the answer, and the reason each is shown next
          to it. A radius built while the resolver is dropping local paths is a
          radius that understates, and a reader deserves to know which one they
          are looking at. */}
      <div>
        <dt>Unresolved local</dt>
        <dd
          className={metrics.unresolved_local_edges > 0 ? styles.summary__warn : undefined}
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
  target,
  onSelect,
}: {
  dependents: readonly GraphNode[];
  target: string;
  onSelect: (path: string) => void;
}) {
  if (dependents.length === 0) {
    return (
      <p className={styles.table__empty}>
        Nothing in this tree imports <code>{target}</code>.
      </p>
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
                rather than a small chevron at the right of it. */}
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

/** The final segment of a path. */
function name(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
