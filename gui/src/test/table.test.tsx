/**
 * The shared data table's sorting, which is the behaviour that made the
 * declarations view worth moving onto it.
 *
 * The table is driven directly rather than through a view, because what is being
 * tested is the table: a header that sorts, a second click that reverses, and a
 * totals row that stays put whatever else moves. A row order read off a rendered
 * table is the only honest way to assert any of that, and it is also how a reader
 * would notice a regression.
 *
 * The first case is the one that is not about sorting at all. The table starts in
 * the report's own order and stays there, because a table that reorders on its
 * first render is a table showing something the report never said -- and the
 * `aria-sort` of `none` is what tells a screen reader the same thing.
 *
 * Every click is inside `act`. That is not decoration: a state update outside
 * `act` is one the DOM has not seen yet, and the row order read straight after the
 * click is the order from before it. The rest of the suite does the same.
 */

import { act, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DataTable, LOC_COLUMNS, type DataRow } from "@/components/DataTable";

/** Three languages, deliberately in an order no sort would produce. */
const ROWS: readonly DataRow[] = [
  { label: "Rust", values: { code: 40, comment: 3, empty: 4, size: 400 } },
  { label: "TOML", values: { code: 4, comment: 0, empty: 1, size: 40 } },
  { label: "Python", values: { code: 12, comment: 2, empty: 6, size: 120 } },
];

const TOTALS: DataRow = {
  label: "Totals",
  values: { code: 56, comment: 5, empty: 11, size: 560 },
};

/** The first column of the body, in the order it is rendered. */
function bodyOrder(): string[] {
  return screen
    .getAllByRole("row")
    .slice(1)
    .map((row) => row.querySelector("td")?.textContent ?? "");
}

/** Render the table, and click a header through `act`. */
function show() {
  render(
    <DataTable
      caption="Lines of code by language"
      firstColumn="Language"
      columns={LOC_COLUMNS}
      rows={ROWS}
      totals={TOTALS}
      emptyMessage="nothing"
      shareOf={(row) => (row.values.code ?? 0) / 56}
    />,
  );
}

/** Click the header bearing `label`, and let the update land. */
function header(label: string): void {
  const node = screen.getByText(label);
  act(() => {
    node.click();
  });
}

describe("DataTable", () => {
  it("keeps the report's own order until a header is clicked", () => {
    show();
    expect(bodyOrder()).toEqual(["Rust", "TOML", "Python", "Totals"]);
  });

  it("says nothing is sorted, in the report's order", () => {
    show();
    expect(
      screen.getByText("Language").closest("th")?.getAttribute("aria-sort"),
    ).toBe("none");
    expect(screen.getByText("Code").closest("th")?.getAttribute("aria-sort")).toBe(
      "none",
    );
  });

  it("sorts by a header, largest first", () => {
    show();
    header("Code");

    // Largest code first, which is the end of "what is most of this" a reader
    // asking is looking for.
    expect(bodyOrder().slice(0, 3)).toEqual(["Rust", "Python", "TOML"]);
    // The totals row is a row of sums, not a row of data, so it does not move.
    expect(bodyOrder()[3]).toBe("Totals");
  });

  it("reverses on a second click of the same header", () => {
    show();
    header("Code");
    expect(bodyOrder().slice(0, 3)).toEqual(["Rust", "Python", "TOML"]);

    header("Code");
    expect(bodyOrder().slice(0, 3)).toEqual(["TOML", "Python", "Rust"]);
  });

  it("opens the label column alphabetical", () => {
    show();
    header("Language");
    // The label is the one column where ascending is the reading order, so it
    // opens low-to-high rather than high-to-low like the numbers.
    expect(bodyOrder().slice(0, 3)).toEqual(["Python", "Rust", "TOML"]);
  });

  it("switches columns when a different header is clicked", () => {
    show();
    header("Empty");
    // Python has the most empty lines: 6, against Rust's 4 and TOML's 1.
    expect(bodyOrder().slice(0, 3)).toEqual(["Python", "Rust", "TOML"]);

    header("Code");
    expect(bodyOrder().slice(0, 3)).toEqual(["Rust", "Python", "TOML"]);
  });

  it("says which column it is sorted by, to a screen reader", () => {
    show();
    const language = screen.getByText("Language").closest("th");
    const code = screen.getByText("Code").closest("th");

    header("Code");
    expect(code?.getAttribute("aria-sort")).toBe("descending");
    expect(language?.getAttribute("aria-sort")).toBe("none");

    header("Code");
    expect(code?.getAttribute("aria-sort")).toBe("ascending");
  });

  it("counts a column a row does not carry as zero", () => {
    // A view whose rows are not all the same shape: the share and the total must
    // not become NaN, and NaN sorts neither first nor last.
    render(
      <DataTable
        caption="a partial table"
        firstColumn="Language"
        columns={[{ key: "code", label: "Code", numeric: true }]}
        rows={[
          { label: "Rust", values: { code: 3 } },
          { label: "TOML", values: {} },
        ]}
        totals={{ label: "Totals", values: { code: 3 } }}
        emptyMessage="nothing"
      />,
    );

    header("Code");
    // TOML's zero is smallest, so it is last.
    expect(bodyOrder()).toEqual(["Rust", "TOML", "Totals"]);
  });
});
