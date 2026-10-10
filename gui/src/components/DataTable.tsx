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
 * The shared table both views render.
 *
 * A `<table>`, not a grid of divs: it is what the data is, and a screen reader
 * announces which row and column a cell belongs to -- which a div grid silently
 * does not. Radix has no table primitive, so this is hand-built and there is
 * nothing to override.
 */
export function DataTable({
  rows,
  totals,
  caption,
  emptyMessage,
  firstColumn,
}: {
  rows: readonly DataRow[];
  totals: DataRow | null;
  caption: string;
  emptyMessage: ReactNode;
  firstColumn: string;
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
              className={column.numeric ? "numeric" : undefined}
            >
              {column.label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.language}>
            <td>{row.language}</td>
            {STAT_COLUMNS.map((column) => (
              <td
                key={column.key}
                className={column.numeric ? "numeric" : undefined}
              >
                {value(row, column).toLocaleString()}
              </td>
            ))}
          </tr>
        ))}
        {totals ? (
          <tr className={styles.table__totals}>
            <td>Totals</td>
            {STAT_COLUMNS.map((column) => (
              <td
                key={column.key}
                className={column.numeric ? "numeric" : undefined}
              >
                {value(totals, column).toLocaleString()}
              </td>
            ))}
          </tr>
        ) : null}
      </tbody>
    </table>
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
