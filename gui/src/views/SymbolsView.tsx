import { AlertTriangle } from "lucide-react";

import { ProgressBar } from "@/components/ProgressBar";
import { useGeneration, useSymbols } from "@/hooks/useAnalysis";
import type { SymbolReport } from "@/lib/ipc";
import styles from "@/styles/views.module.scss";

/** The kinds in the order the CLI reports them, and the label each gets. */
const KINDS = [
  { kind: "functions", label: "Functions" },
  { kind: "types", label: "Types" },
  { kind: "enums", label: "Enums" },
  { kind: "constants", label: "Constants" },
] as const;

/**
 * The declarations view: what each language declares, read from parse trees.
 *
 * `sephera symbols` already counts and already totals, so this renders the same
 * rows the CLI prints. The reason to care in a GUI is the *shape*: a language's
 * four counts side by side say how a codebase divides between behaviour and
 * vocabulary, which a line count cannot.
 */
export function SymbolsView() {
  const { data, error, busy } = useSymbols();
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
  // Counting with nothing to show yet: a header and an indeterminate bar.
  if (busy && !data) {
    return (
      <div className={styles.view}>
        <header className={styles.view__header}>
          <h2 className={styles.view__title}>Declarations</h2>
          <p className={styles.view__subtitle}>Reading declarations…</p>
        </header>
        <ProgressBar label="Counting declarations" />
      </div>
    );
  }
  if (!data) return null;
  void generation;

  return (
    <div className={styles.view}>
      <header className={styles.view__header}>
        <h2 className={styles.view__title}>Declarations</h2>
        <p className={styles.view__subtitle}>
          {data.files_scanned.toLocaleString()} files parsed
          {data.files_skipped > 0
            ? ` | ${data.files_skipped.toLocaleString()} skipped`
            : ""}{" "}
          | {data.languages_detected.toLocaleString()} languages
        </p>
      </header>

      {data.by_language.length === 0 ? (
        <p className={styles.table__empty}>No declarations found.</p>
      ) : (
        <table className={styles.table}>
          <caption className="sr-only">Declarations by language</caption>
          <thead>
            <tr>
              <th scope="col">Language</th>
              {KINDS.map((entry) => (
                <th key={entry.kind} scope="col" className="numeric">
                  {entry.label}
                </th>
              ))}
              <th scope="col" className="numeric">
                Total
              </th>
            </tr>
          </thead>
          <tbody>
            {data.by_language.map((language) => (
              <tr key={language.language}>
                <td>{language.language}</td>
                {KINDS.map((entry) => (
                  <td key={entry.kind} className="numeric">
                    {(language.counts[entry.kind] ?? 0).toLocaleString()}
                  </td>
                ))}
                <td className="numeric">
                  {total(language).toLocaleString()}
                </td>
              </tr>
            ))}
            <tr className={styles.table__totals}>
              <td>Totals</td>
              {KINDS.map((entry) => (
                <td key={entry.kind} className="numeric">
                  {(data.totals[entry.kind] ?? 0).toLocaleString()}
                </td>
              ))}
              <td className="numeric">
                {KINDS.reduce(
                  (sum, entry) => sum + (data.totals[entry.kind] ?? 0),
                  0,
                ).toLocaleString()}
              </td>
            </tr>
          </tbody>
        </table>
      )}
    </div>
  );
}

/** A language's four kinds summed, for its own total column. */
function total(language: SymbolReport["by_language"][number]): number {
  return KINDS.reduce(
    (sum, entry) => sum + (language.counts[entry.kind] ?? 0),
    0,
  );
}
