import type { ReactNode } from "react";
import styles from "@/styles/table.module.scss";

/**
 * One row of the shared data table.
 *
 * `files` is deliberately absent. It is a property of the whole scan, not of a
 * language -- the CLI's report has no per-language file count at all -- so a row
 * that carries one is a row that has to invent a number to put there. The
 * line-count view passes the scan's own count as a separate figure instead.
 */
export interface DataRow {
  language: string;
  code: number;
  comment: number;
  empty: number;
  bytes: number;
}

/** A column, in the order it should render. */
export interface Column {
  key: string;
  label: string;
  numeric?: boolean;
}

/** The column order every statistics view shares. */
export const STAT_COLUMNS: readonly Column[] = [
  { key: "code", label: "Code", numeric: true },
  { key: "comment", label: "Comment", numeric: true },
  { key: "empty", label: "Empty", numeric: true },
  { key: "bytes", label: "Bytes", numeric: true },
];

/**
 * The shared table both statistics views render.
 *
 * A `<table>`, not a grid of divs: it is what the data is, and a screen reader
 * announces which row and column a cell belongs to -- which a div grid silently
 * does not. Radix has no table primitive, so this is hand-built and there is
 * nothing to override.
 *
 * `shareOf` is the one column that is not the same for every view. The line count
 * shows what fraction of the code each language is; the declarations view shares
 * a different total. It is a function of the row rather than a key, so the two
 * views can share the component without the component knowing which total is
 * being divided by.
 */
export function DataTable({
  rows,
  totals,
  caption,
  emptyMessage,
  firstColumn,
  shareOf,
  shareLabel,
}: {
  rows: readonly DataRow[];
  totals: DataRow | null;
  caption: string;
  emptyMessage: ReactNode;
  firstColumn: string;
  /** A row's share of the total, 0..1, or `null` for no share column. */
  shareOf?: (row: DataRow) => number | null;
  shareLabel?: string;
}) {
  if (rows.length === 0) {
    return <p className={styles.table__empty}>{emptyMessage}</p>;
  }

  return (
    <table className={styles.table}>
      <caption className="sr-only">{caption}</caption>
      <thead>
        <tr>
          <th scope="col">{firstColumn}</th>
          {STAT_COLUMNS.map((column) => (
            <th
              key={column.key}
              scope="col"
              className={column.numeric ? styles.numeric : undefined}
            >
              {column.label}
            </th>
          ))}
          {shareOf ? (
            <th scope="col" className={styles.numeric}>
              {shareLabel ?? "Share"}
            </th>
          ) : null}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.language}>
            <td>{row.language}</td>
            {STAT_COLUMNS.map((column) => (
              <td
                key={column.key}
                className={column.numeric ? styles.numeric : undefined}
              >
                {value(row, column).toLocaleString()}
              </td>
            ))}
            {shareOf ? <ShareCell row={row} shareOf={shareOf} /> : null}
          </tr>
        ))}
        {totals ? (
          <tr className={styles.table__totals}>
            <td>Totals</td>
            {STAT_COLUMNS.map((column) => (
              <td
                key={column.key}
                className={column.numeric ? styles.numeric : undefined}
              >
                {value(totals, column).toLocaleString()}
              </td>
            ))}
            {shareOf ? (
              <td className={styles.numeric}>
                {((shareOf(totals) ?? 0) * 100).toFixed(1)}%
              </td>
            ) : null}
          </tr>
        ) : null}
      </tbody>
    </table>
  );
}

/** One share cell: the percentage, with the bar behind it. */
function ShareCell({
  row,
  shareOf,
}: {
  row: DataRow;
  shareOf: (row: DataRow) => number | null;
}) {
  const share = shareOf(row) ?? 0;
  return (
    <td className={`${styles.numeric} ${styles.cell}`}>
      {/* The bar is a background on the cell rather than a child, so it adds no
          height and cannot disturb the row's metrics. */}
      <span
        className={styles.cell__bar}
        style={{ width: `${Math.min(share, 1) * 100}%` }}
        aria-hidden="true"
      />
      <span>{(share * 100).toFixed(1)}%</span>
    </td>
  );
}

/** The cell's value, by column key. */
function value(row: DataRow, column: Column): number {
  switch (column.key) {
    case "code":
      return row.code;
    case "comment":
      return row.comment;
    case "empty":
      return row.empty;
    default:
      return row.bytes;
  }
}
