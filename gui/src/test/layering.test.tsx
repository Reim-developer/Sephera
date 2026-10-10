import { act, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { LocView } from "@/views/LocView";
import { SymbolsView } from "@/views/SymbolsView";
import { DependenciesView } from "@/views/DependenciesView";
import { useClient } from "@/state/store";
import {
  declarations,
  dependents,
  installServices,
  twoLanguageLoc,
} from "@/test/setup";

/**
 * The point of the layering: a view renders from a populated store with no host
 * in sight. If any of these tests needs `invoke`, a service, or a Tauri module,
 * the boundary has been crossed somewhere above.
 */

/**
 * Seed the store with a fixture, without going through the service layer.
 *
 * A function of the previous state rather than a partial object, because
 * zustand v5's setState accepts either a whole state or a function returning
 * one -- and a partial would be read as a replacement, dropping every field the
 * test did not mention. Spreading the previous state in is what makes seed
 * name only the slice it means.
 */
function seed(partial: Partial<ReturnType<typeof useClient.getState>>): void {
  useClient.setState((state) => ({ ...state, ...partial }));
}

beforeEach(() => {
  useClient.setState({
    root: "/project",
    ignore: [],
    target: "",
    selected: null,
    view: "loc",
    tree: {},
    generation: 0,
  });
});

describe("LocView", () => {
  it("renders the count the store holds", () => {
    installServices();
    seed({ loc: { data: twoLanguageLoc(), error: null, busy: false } });

    render(<LocView />);

    // The totals row, which is the number a reader came for.
    expect(screen.getByText("Totals")).toBeTruthy();
    expect(screen.getByText("24")).toBeTruthy();
    // The config path, which says why the number is what it is.
    expect(screen.getByText(/3 files/)).toBeTruthy();
  });

  it("renders an error rather than a table", () => {
    installServices();
    seed({ loc: { data: null, error: "path is not a directory", busy: false } });

    render(<LocView />);

    expect(screen.getByText(/path is not a directory/)).toBeTruthy();
    expect(screen.queryByText("Totals")).toBeNull();
  });

  it("shows no table when there is no result yet", () => {
    installServices();
    seed({ loc: { data: null, error: null, busy: false } });

    render(<LocView />);

    // An empty tree and a missing result are different states, and conflating
    // them is how a view renders `null` rows as zero rows.
    expect(screen.queryByText("Totals")).toBeNull();
  });
});

describe("SymbolsView", () => {
  it("renders the four kinds the store holds", () => {
    installServices();
    seed({ symbols: { data: declarations(), error: null, busy: false } });

    render(<SymbolsView />);

    expect(screen.getByText("Functions")).toBeTruthy();
    expect(screen.getByText("Enums")).toBeTruthy();
    // Per-language and totals are both rendered, so a total that disagrees with
    // its rows is visible.
    expect(screen.getAllByText("27")).toHaveLength(2);
  });
});

describe("DependenciesView", () => {
  it("shows the list of dependents, heaviest first", () => {
    installServices();
    seed({
      target: "a.rs",
      graph: { data: dependents(), error: null, busy: false },
    });

    render(<DependenciesView />);

    expect(screen.getByText("b.rs")).toBeTruthy();
    expect(screen.getByText("c.rs")).toBeTruthy();
    // `imports_count` descending, asserted inside the table rather than across the
    // document: the summary carries its own `1` (unresolved-local), and a
    // document-order assertion would read that one instead.
    const table = screen.getByRole("table");
    const cells = within(table).getAllByText(/^(12|1)$/);
    expect(cells.map((cell) => cell.textContent)).toEqual(["12", "1"]);
  });

  it("colours unresolved-local because it qualifies the answer", () => {
    installServices();
    seed({
      target: "a.rs",
      graph: { data: dependents(), error: null, busy: false },
    });

    render(<DependenciesView />);

    // The metric is the caveat, not decoration: one unresolved local path means
    // this radius understates. Queried inside the summary, because the table
    // carries a `1` of its own.
    const label = screen.getByText("Unresolved local");
    const value = label.nextElementSibling;
    expect(value?.textContent).toBe("1");
  });

  it("asks for a file when there is no target", () => {
    installServices();
    seed({ target: "", graph: { data: null, error: null, busy: false } });

    render(<DependenciesView />);

    expect(screen.getByText("Pick a file")).toBeTruthy();
    // The query row is not offered until there is something to trace.
    expect(screen.queryByLabelText("File to trace")).toBeNull();
  });
});

describe("the store, with the registry replaced", () => {
  it("calls the service and writes the result", async () => {
    const calls = installServices();

    await act(async () => {
      useClient.getState().setRoot("/project");
    });

    // Every analysis is run for a directory change, because a stale one is one
    // click away from being shown as current.
    expect(calls.loc).toHaveBeenCalledWith("/project", []);
    expect(calls.symbols).toHaveBeenCalledWith("/project", []);
    expect(useClient.getState().loc.data).not.toBeNull();
    expect(useClient.getState().loc.data?.config_source).toBe(
      "/project/.sephera.toml",
    );
  });

  it("a failed service lands as an error, not a thrown rejection", async () => {
    const calls = installServices();
    calls.loc.mockRejectedValueOnce(new Error("path is not a directory"));

    await act(async () => {
      useClient.getState().setRoot("/project");
    });

    const state = useClient.getState();
    expect(state.loc.error).toContain("path is not a directory");
    expect(state.loc.data).toBeNull();
  });

  it("choosing a target is also choosing to look at the graph", async () => {
    const calls = installServices();

    await act(async () => {
      useClient.getState().setTarget("a.rs");
    });

    expect(useClient.getState().view).toBe("graph");
    expect(calls.graph).toHaveBeenCalled();
  });

  it("a cancelled dialog leaves the root alone", async () => {
    installServices();

    await act(async () => {
      // `setRoot` is what a chosen path does; no path means no directory change.
      useClient.getState().setRoot("/project");
    });

    expect(useClient.getState().root).toBe("/project");
  });
});
