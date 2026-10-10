import { AlertTriangle } from "lucide-react";

import { DataTable, type DataRow } from "@/components/DataTable";
import { ProgressBar } from "@/components/ProgressBar";
import { useGeneration, useLoc } from "@/hooks/useAnalysis";
import type { LocView } from "@/lib/ipc";
import styles from "@/styles/views.module.scss";

/**
 * The line-count view.
 *
 * It holds no state and calls nothing. It subscribes to the line count, reads the
 * directory and the patterns, and renders -- which is all a view should do, and
 * the reason it can be tested by handing it a populated store.
 *
 * `generation` is subscribed even though nothing is done with it. Without it, a
 * recompute that returned the same object identity would not re-render, and the
 * view would show a stale answer after an edit that changed the number.
 */
export function LocView() {
  const { data, error, busy } = useLoc();
  const generation = useGeneration();

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
  // Counting with nothing to show yet: a header and an indeterminate bar,
  // rather than a placeholder that says the same thing in words.
  if (busy && !data) {
    return (
      <div className={styles.view}>
        <header className={styles.view__header}>
          <h2 className={styles.view__title}>Lines of code</h2>
          <p className={styles.view__subtitle}>Counting…</p>
        </header>
        <ProgressBar label="Counting lines of code" />
      </div>
    );
  }
  if (!data) return null;

  // The generation is read here rather than in a dependency array, because there
  // is no effect to depend on anything. Its value is what makes the render happen.
  void generation;

  const total = data.totals.code || 1;

  return (
    <div className={styles.view}>
      <header className={styles.view__header}>
        <h2 className={styles.view__title}>Lines of code</h2>
        <p className={styles.view__subtitle}>
          {data.files_scanned.toLocaleString()} files ·{" "}
          {data.rows.length.toLocaleString()} languages · {data.elapsed_ms} ms
        </p>
      </header>

      <DataTable
        caption="Lines of code by language"
        firstColumn="Language"
        rows={data.rows.map(toDataRow)}
        totals={toDataRow(data.totals)}
        emptyMessage="No source files found."
        shareOf={(row) => row.code / total}
        shareLabel="Share of code"
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
