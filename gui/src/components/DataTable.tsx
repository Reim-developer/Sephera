import { useMemo, useState, type ReactNode } from "react";
import styles from "@/styles/table.module.scss";

/** One row of the shared data table.
 *
 * Two fields rather than one per column, and the reason is the totals row. A view
 * that had a `code` field and a `functions` field would need the table to know
 * which one it is rendering, which is the table knowing about a view. So a row
 * says what it is (`label`) and what its numbers are (`values`), and the columns
 * are the view's to choose.
 *
 * `files` is still absent, and still for the same reason: it is a property of the
 * whole scan rather than of a language, so a row carrying one is a row inventing a
 * number to put there.
 */
export interface DataRow {
  /** The first column, which is the row's identity. */
  label: string;
  /** Every numeric column, by the key the column declares. */
  values: Record<string, number>;
}

/** A column, in the order it should render. */
export interface Column {
  /** The key in a row's `values`. */
  key: string;
  label: string;
  numeric?: boolean;
}

/** The column order the line-count view renders.
 *
 * The line count is the view the shape was taken from: code, comment, empty and
 * size are what a language's lines divide into, and the declarations view maps
 * its four kinds onto the same four slots plus a total rather than inventing a
 * second shape. */
export const LOC_COLUMNS: readonly Column[] = [
  { key: "code", label: "Code", numeric: true },
  { key: "comment", label: "Comment", numeric: true },
  { key: "empty", label: "Empty", numeric: true },
  { key: "size", label: "Bytes", numeric: true },
];

/** The declarations view's columns: the four kinds, and their sum. */
export const SYMBOL_COLUMNS: readonly Column[] = [
  { key: "functions", label: "Functions", numeric: true },
  { key: "types", label: "Types", numeric: true },
  { key: "enums", label: "Enums", numeric: true },
  { key: "constants", label: "Constants", numeric: true },
  { key: "total", label: "Total", numeric: true },
];

/** The key the label column sorts by.
 *
 * A sentinel rather than the empty string, because a row's `values` could
 * legitimately hold a key the caller chose for the first column, and two columns
 * answering to one name is how a sort comes to sort the wrong one. */
const LABEL_KEY = "@label";

/**
 * How a table is sorted, or `null` when it is not.
 *
 * `null` is the starting state and it is not a lazy default: the report has
 * already decided its own order -- languages in whatever order the scan produced
 * them -- and a table that reorders on its first render is a table showing
 * something the report never said. The reader's first click is the first time a
 * sort happens.
 */
type Sort = { key: string; descending: boolean } | null;

/**
 * The shared table the statistics views render.
 *
 * A `<table>`, not a grid of divs: it is what the data is, and a screen reader
 * announces which row and column a cell belongs to -- which a div grid silently
 * does not. Radix has no table primitive, so this is hand-built and there is
 * nothing to override.
 *
 * Sortable, because a language table with twelve rows and no way to reorder them
 * is a table the reader has to scan by eye. Clicking a header sorts; clicking it
 * again reverses. A numeric column opens at its largest, which is the end of
 * "what is most of this" the reader is looking for, and the label column opens
 * alphabetical. `aria-sort` carries the state to a screen reader, since the arrow
 * alone is a picture -- and it says `none` until a click happens, which is what
 * tells one the report's order is still in force.
 *
 * `shareOf` is the one column that is not the same for every view. The line count
 * shows what fraction of the code each language is; the declarations view shares a
 * different total. It is a function of the row rather than a key, so the two views
 * share the component without the component knowing which total is being divided
 * by.
 */
export function DataTable({
  rows,
  totals,
  caption,
  emptyMessage,
  firstColumn,
  columns,
  shareOf,
  shareLabel,
  sortable = true,
}: {
  rows: readonly DataRow[];
  totals: DataRow | null;
  caption: string;
  emptyMessage: ReactNode;
  firstColumn: string;
  columns: readonly Column[];
  /** A row's share of the total, 0..1, or `null` for no share column. */
  shareOf?: (row: DataRow) => number | null;
  shareLabel?: string;
  sortable?: boolean;
}) {
  const [sort, setSort] = useState<Sort>(null);

  const ordered = useMemo(() => order(rows, sort), [rows, sort]);

  if (rows.length === 0) {
    return <p className={styles.table__empty}>{emptyMessage}</p>;
  }

  /** A click on a column's header: sort by it, or reverse if it already is. */
  function onSort(key: string) {
    setSort((current) =>
      current?.key === key
        ? { key, descending: !current.descending }
        : // A numeric column opens at its largest; the label column opens
          // alphabetical. The label is the one column where ascending is the
          // reading order, so it is the exception rather than the rule.
          { key, descending: key !== LABEL_KEY },
    );
  }

  const ariaSort = (key: string): "ascending" | "descending" | "none" => {
    if (sort?.key !== key) return "none";
    return sort.descending ? "descending" : "ascending";
  };

  return (
    <table className={styles.table}>
      <caption className="sr-only">{caption}</caption>
      <thead>
        <tr>
          <Header
            label={firstColumn}
            sortKey={LABEL_KEY}
            sortable={sortable}
            aria={ariaSort(LABEL_KEY)}
            onSort={onSort}
            first
          />
          {columns.map((column) => (
            <Header
              key={column.key}
              label={column.label}
              sortKey={column.key}
              sortable={Boolean(sortable && column.numeric)}
              aria={ariaSort(column.key)}
              onSort={onSort}
              numeric={column.numeric}
            />
          ))}
          {shareOf ? (
            <th scope="col" className={styles.numeric}>
              {shareLabel ?? "Share"}
            </th>
          ) : null}
        </tr>
      </thead>
      <tbody>
        {ordered.map((row) => (
          <tr key={row.label}>
            <td>{row.label}</td>
            {columns.map((column) => (
              <td
                key={column.key}
                className={column.numeric ? styles.numeric : undefined}
              >
                {value(row, column.key).toLocaleString()}
              </td>
            ))}
            {shareOf ? <ShareCell row={row} shareOf={shareOf} /> : null}
          </tr>
        ))}
        {totals ? (
          <tr className={styles.table__totals}>
            <td>Totals</td>
            {columns.map((column) => (
              <td
                key={column.key}
                className={column.numeric ? styles.numeric : undefined}
              >
                {value(totals, column.key).toLocaleString()}
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

/**
 * One header cell, a button when it can be clicked and text when it cannot.
 *
 * A `<button>` rather than a click handler on the `<th>`, because a click handler
 * on a `<th>` is announced to nobody: no focus, no keyboard, no role. The button
 * carries all three and costs nothing -- and it makes the arrow its own child, so
 * the label and the direction are one control.
 */
function Header({
  label,
  sortKey,
  sortable,
  aria,
  onSort,
  numeric,
  first,
}: {
  label: string;
  sortKey: string;
  sortable: boolean;
  aria: "ascending" | "descending" | "none";
  onSort: (key: string) => void;
  numeric?: boolean;
  first?: boolean;
}) {
  const clickable = Boolean(sortable && (first || numeric));

  return (
    <th
      scope="col"
      className={numeric ? styles.numeric : undefined}
      aria-sort={aria}
    >
      {clickable ? (
        <button
          type="button"
          className={styles.head__button}
          onClick={() => onSort(sortKey)}
        >
          {label}
          <Arrow direction={aria} />
        </button>
      ) : (
        label
      )}
    </th>
  );
}

/** The rows in the order `sort` asks for, or the report's own when it is `null`. */
function order(rows: readonly DataRow[], sort: Sort): DataRow[] {
  if (!sort) return [...rows];

  const sorted = [...rows];
  if (sort.key === LABEL_KEY) {
    sorted.sort((left, right) => left.label.localeCompare(right.label));
  } else {
    sorted.sort((left, right) => value(left, sort.key) - value(right, sort.key));
  }
  if (sort.descending) sorted.reverse();
  return sorted;
}

/** The cell's value, by the column key it was asked for.
 *
 * A column a row does not carry counts as zero rather than as NaN: a partial row
 * and an absent row are the same number, and NaN would sort nowhere. */
function value(row: DataRow, key: string): number {
  return row.values[key] ?? 0;
}

/** The direction a column is sorted in, as an arrow.
 *
 * A glyph rather than an icon component, because it is one character that changes
 * with the state and an `aria-sort` on the header is what a screen reader reads.
 * An un-sorted column still gets a faint marker, so the header reads as a control
 * rather than as text that happens to be clickable. */
function Arrow({ direction }: { direction: "ascending" | "descending" | "none" }) {
  const glyph = direction === "ascending" ? "↑" : direction === "descending" ? "↓" : "↕";
  return (
    <span className={styles.head__arrow} aria-hidden="true">
      {glyph}
    </span>
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
