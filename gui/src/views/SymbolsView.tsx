import { AlertTriangle } from "lucide-react";

import {
  DataTable,
  SYMBOL_COLUMNS,
  type DataRow,
} from "@/components/DataTable";
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
 *
 * It renders through the shared `DataTable`, which it did not do before. The view
 * hand-built its own `<table>` -- thirteen lines of header and cell markup -- and
 * so had no frozen header, no share bar, and no way to reorder the languages. All
 * three come from the component now, and the markup that got the table is the
 * mapping that got deleted.
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

  // The generation is read here rather than in a dependency array, because there
  // is no effect to depend on anything. Its value is what makes the render happen.
  void generation;

  // The share is of *declarations*, not of code: a language holding a quarter of
  // every `fn` in the tree is a different claim from a quarter of its lines, and
  // conflating the two is how a share column comes to lie. The table never learns
  // which total it is dividing by.
  const all = sumOf(data.totals) || 1;

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

      <DataTable
        caption="Declarations by language"
        firstColumn="Language"
        columns={SYMBOL_COLUMNS}
        rows={data.by_language.map(toLanguageRow)}
        totals={{
          label: "Totals",
          values: valuesOf(data.totals),
        }}
        emptyMessage="No declarations found."
        shareOf={(row) => (row.values.total ?? 0) / all}
        shareLabel="Share of declarations"
      />
    </div>
  );
}

/** The four kinds summed, for the share divisor. */
function sumOf(counts: Partial<Record<string, number>>): number {
  return KINDS.reduce((sum, { kind }) => sum + (counts[kind] ?? 0), 0);
}

/** A per-language row in the shape the shared table renders. */
function toLanguageRow(
  language: SymbolReport["by_language"][number],
): DataRow {
  return { label: language.language, values: valuesOf(language.counts) };
}

/**
 * The four kinds, plus their sum, in the shape the table reads.
 *
 * `total` is a fifth key rather than something the table computes, because the
 * table has no opinion about what a row's numbers sum to -- and the share column
 * needs it as a number it can divide, not as a heading it would have to re-add.
 */
function valuesOf(counts: Partial<Record<string, number>>): Record<string, number> {
  const values: Record<string, number> = {};
  let sum = 0;
  for (const { kind } of KINDS) {
    const count = counts[kind] ?? 0;
    values[kind] = count;
    sum += count;
  }
  values.total = sum;
  return values;
}

