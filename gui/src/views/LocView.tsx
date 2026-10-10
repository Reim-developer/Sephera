import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";

import { DataTable, type DataRow } from "../components/DataTable";
import type { LocView } from "../lib/ipc";
import { countLines } from "../lib/ipc";
import styles from "../styles/views.module.scss";

/**
 * The line-count view: one table, ordered the way the CLI orders it.
 *
 * The Rust side already sorted the rows by code lines and summed the totals, so
 * this renders in the order it is given rather than re-deriving either. A second
 * sort here would be a second opinion to disagree with.
 *
 * `reloadToken` is how the app asks for a recompute: a changed value means
 * "again". It is a token rather than a callback identity because the tree is
 * deep enough that a new function on every render would re-run this effect and
 * hit Rust on a state change that has nothing to do with the count.
 */
export function LocView({
  path,
  ignore,
  reloadToken,
}: {
  path: string;
  ignore: string[];
  reloadToken: number;
}) {
  const [view, setView] = useState<LocView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);

  const run = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setView(await countLines(path, ignore));
    } catch (cause) {
      setError(String(cause));
      setView(null);
    } finally {
      setBusy(false);
    }
  }, [path, ignore]);

  useEffect(() => {
    void run();
  }, [run, reloadToken]);

  if (error) {
    return (
      <div className={styles.banner}>
        <span className={styles.banner__icon} aria-hidden="true">
          <AlertTriangle size={16} />
        </span>
        <span>{error}</span>
      </div>
    );
  }
  if (busy && !view) {
    return (
      <div className={styles.placeholder}>
        <span className={styles.placeholder__title}>
          <RefreshCw size={14} aria-hidden="true" /> Counting…
        </span>
      </div>
    );
  }
  if (!view) return null;

  return (
    <div className={styles.view}>
      <header className={styles.view__header}>
        <h2 className={styles.view__title}>Lines of code</h2>
        <p className={styles.view__subtitle}>
          {view.files_scanned.toLocaleString()} files ·{" "}
          {view.rows.length.toLocaleString()} languages · {view.elapsed_ms} ms
        </p>
      </header>

      <DataTable
        caption="Lines of code by language"
        firstColumn="Language"
        rows={view.rows.map(toDataRow)}
        totals={toDataRow(view.totals)}
        emptyMessage="No source files found."
      />
    </div>
  );
}

/** A view-model row in the shape the shared table renders. */
function toDataRow(row: LocView["rows"][number]): DataRow {
  return {
    language: row.language,
    code: row.code,
    comment: row.comment,
    empty: row.empty,
    bytes: row.size_bytes,
  };
}
