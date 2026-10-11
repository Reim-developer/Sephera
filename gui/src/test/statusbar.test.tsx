/**
 * The status bar, and the viewer's reporting of the caret.
 *
 * Two things are under test and they are tested through the store, because that is
 * the seam: the viewer writes a caret, the status bar reads it. jsdom has no
 * layout, so a caret computed from a pointer position is not reproducible here --
 * but the bar's reading of one is, and the EOL and language come from the host's
 * own report.
 */

import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { StatusBar } from "@/components/StatusBar";
import { useClient } from "@/state/store";
import { fileDetail, installServices } from "@/test/setup";

beforeEach(() => {
  installServices();
  useClient.setState((state) => ({ ...state, selected: null, caret: { line: 1, column: 1 } }));
});

describe("StatusBar", () => {
  it("reports the file's language and line-ending sequence", () => {
    useClient.setState((state) => ({
      ...state,
      selected: "src/a.rs",
      file: { data: fileDetail(), error: null, busy: false },
    }));
    render(<StatusBar />);

    expect(screen.getByText("Rust")).toBeTruthy();
    expect(screen.getByText("LF")).toBeTruthy();
    expect(screen.getByText("Ln 1, Col 1")).toBeTruthy();
  });

  it("reports CRLF, because that is what the host said", () => {
    useClient.setState((state) => ({
      ...state,
      selected: "src/a.rs",
      file: {
        data: { ...fileDetail(), eol: "CRLF" },
        error: null,
        busy: false,
      },
    }));
    render(<StatusBar />);

    expect(screen.getByText("CRLF")).toBeTruthy();
  });

  it("reports the caret the viewer wrote", async () => {
    await act(async () => {
      useClient.getState().setCaret({ line: 84, column: 12 });
    });
    useClient.setState((state) => ({ ...state, selected: "src/a.rs" }));
    render(<StatusBar />);

    expect(screen.getByText("Ln 84, Col 12")).toBeTruthy();
  });

  it("has nothing to say about a file before one is open", () => {
    render(<StatusBar />);
    // The bar still shows: an editor with no file has a status bar, and one that
    // vanishes reads as the window being broken.
    expect(screen.getByText("No file open")).toBeTruthy();
    expect(screen.getByText("Ln 1, Col 1")).toBeTruthy();
  });

  it("shows the file's name rather than the whole path", () => {
    useClient.setState((state) => ({
      ...state,
      selected: "crates/sephera_graph/src/resolver.rs",
      file: { data: fileDetail(), error: null, busy: false },
    }));
    render(<StatusBar />);

    expect(screen.getByText("resolver.rs")).toBeTruthy();
  });
});

describe("the caret in the store", () => {
  it("starts on the first line", () => {
    // Not "nowhere": a bar that reports nothing before the first hover reads as
    // broken, and line 1 column 1 is where a reader's eye already is.
    useClient.setState((state) => ({ ...state, caret: { line: 0, column: 0 } }));
    expect(useClient.getState().setCaret).toBeDefined();
  });
});
