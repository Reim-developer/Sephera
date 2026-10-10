import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { LocView } from "@/views/LocView";
import { SymbolsView } from "@/views/SymbolsView";
import { useClient } from "@/state/store";
import {
  declarations,
  installServices,
  twoLanguageLoc,
} from "@/test/setup";

/**
 * The point of the layering: a view renders from a populated store with no host
 * in sight. If any of these tests needed `invoke`, a service, or a Tauri module,
 * the boundary would have been crossed somewhere above them.
 */

/** Seed the store with a fixture, without going through the service layer.
 *
 * A function of the previous state rather than a partial object, because
 * zustand v5's `setState` accepts either a whole state or a function returning
 * one -- and a partial would be read as a replacement, dropping every field the
 * test did not mention. Spreading the previous state in is what makes `seed`
 * name only the slice it means.
 */
function seed(partial: Partial<ReturnType<typeof useClient.getState>>): void {
  useClient.setState((state) => ({ ...state, ...partial }));
}

beforeEach(() => {
  useClient.setState({
    root: ".",
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

describe("the store, with the registry replaced", () => {
  it("calls the service and writes the result", async () => {
    const calls = installServices();

    await act(async () => {
      useClient.getState().setRoot(".");
    });

    // Every analysis is run for a directory change, because a stale one is one
    // click away from being shown as current.
    expect(calls.loc).toHaveBeenCalledWith(".", [], expect.any(Number));
    expect(calls.symbols).toHaveBeenCalledWith(".", [], expect.any(Number));
    expect(useClient.getState().loc.data).not.toBeNull();
    expect(useClient.getState().loc.data?.config_source).toBe(
      "/project/.sephera.toml",
    );
  });

  it("a failed service lands as an error, not a thrown rejection", async () => {
    const calls = installServices();
    calls.loc.mockRejectedValueOnce(new Error("path is not a directory"));

    await act(async () => {
      useClient.getState().setRoot(".");
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

  it("selecting a file counts that file, and only that one", async () => {
    const calls = installServices();

    await act(async () => {
      useClient.getState().setSelected("src/a.rs");
    });

    // The store asked for the file, and did not re-run the directory counts to
    // answer a question only the file can answer.
    expect(calls.file).toHaveBeenCalledWith(".", "src/a.rs");
    expect(useClient.getState().file.data?.code).toBe(20);
    expect(useClient.getState().target).toBe("src/a.rs");
  });

  it("cancel discards a reply that arrives after it", async () => {
    const calls = installServices();

    // A reply that lands after the cancel, so the epoch comparison is the only
    // thing that stops it. A store that trusted its reply would be a store where
    // Cancel only closes the dialog and the number still arrives.
    calls.loc.mockImplementation(
      async (_root: unknown, _ignore: unknown, epoch: number) =>
        new Promise<[unknown, number]>((resolve) => {
          setTimeout(() => resolve([twoLanguageLoc(), epoch]), 0);
        }),
    );

    await act(async () => {
      useClient.getState().recompute();
    });
    expect(useClient.getState().loc.busy).toBe(true);

    await act(async () => {
      useClient.getState().cancel();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    // The host was told, the dialog closed, and the late reply changed nothing.
    expect(calls.cancel).toHaveBeenCalled();
    expect(useClient.getState().loc.busy).toBe(false);
    expect(useClient.getState().loc.data).toBeNull();
  });
});
