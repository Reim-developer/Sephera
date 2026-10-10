import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, Braces } from "lucide-react";

import {
  countDeclarations,
  type LanguageSymbols,
  type SymbolKind,
  type SymbolReport,
} from "../lib/ipc";
import styles from "../styles/views.module.scss";

/** The kinds in the order the CLI reports them, and the label each gets. */
const KINDS: ReadonlyArray<{ kind: SymbolKind; label: string }> = [
  { kind: "functions", label: "Functions" },
  { kind: "types", label: "Types" },
  { kind: "enums", label: "Enums" },
  { kind: "constants", label: "Constants" },
];
/**
 * The declarations view: what each language declares, read from parse trees.
 *
 * `sephera symbols` already counts and already totals, so this renders the same
 * rows the CLI prints. The reason to care at all in a GUI is the *shape*: a
 * language's four counts side by side say how a codebase divides between
 * behaviour and vocabulary, which a line count cannot.
 */
export function SymbolsView({
  path,
  ignore,
  reloadToken,
}: {
  path: string;
  ignore: string[];
  reloadToken: number;
}) {
  const [report, setReport] = useState<SymbolReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);

  const run = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setReport(await countDeclarations(path, ignore));
    } catch (cause) {
      setError(String(cause));
      setReport(null);
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
  if (busy && !report) {
    return (
      <div className={styles.placeholder}>
        <span className={styles.placeholder__title}>
          <Braces size={14} aria-hidden="true" /> Reading declarations…
        </span>
      </div>
    );
  }
  if (!report) return null;

  return (
    <div className={styles.view}>
      <header className={styles.view__header}>
        <h2 className={styles.view__title}>Declarations</h2>
        <p className={styles.view__subtitle}>
          {report.files_scanned.toLocaleString()} files parsed
          {report.files_skipped > 0
            ? ` · ${report.files_skipped.toLocaleString()} skipped`
            : ""}{" "}
          · {report.languages_detected.toLocaleString()} languages
        </p>
      </header>

      {report.by_language.length === 0 ? (
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
            {report.by_language.map((language) => (
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
                  {(report.totals[entry.kind] ?? 0).toLocaleString()}
                </td>
              ))}
              <td className="numeric">
                {KINDS.reduce(
                  (sum, entry) => sum + (report.totals[entry.kind] ?? 0),
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
function total(language: LanguageSymbols): number {
  return KINDS.reduce(
    (sum, entry) => sum + (language.counts[entry.kind] ?? 0),
    0,
  );
}
