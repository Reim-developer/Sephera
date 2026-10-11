/**
 * The workbench's shell, rendered against a store.
 *
 * Three things a reader reported, and each is a state the shell is in rather than a
 * value it computes: the file's text replaces the three directory analyses, so the
 * tabs that choose between them come down; the folder and the pattern field stay,
 * because they apply whatever is selected; and the count button still recounts.
 *
 * The layout contract itself is in `layout.test.ts`; this is about which controls
 * exist in which state.
 */

import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { TopNav } from "@/components/TopNav";
import { useClient } from "@/state/store";
import { withServices } from "@/services/registry";

/** The shell's controls, or `null` when one is not rendered. */
function control(label: string | RegExp): HTMLElement | null {
  return screen.queryByText(label);
}

beforeEach(() => {
  withServices({});
  useClient.setState((state) => ({
    ...state,
    root: "/project",
    selected: null,
    view: "loc",
  }));
});

describe("the workbench shell", () => {
  it("shows the three analyses while nothing is selected", () => {
    render(<TopNav />);
    // The three tabs, and the controls that apply to the whole folder.
    expect(control("Lines of code")).toBeTruthy();
    expect(control("Declarations")).toBeTruthy();
    expect(control("Dependencies")).toBeTruthy();
    expect(control("Scan folder…")).toBeTruthy();
  });

  it("hides the analyses while a file is open", () => {
    // The panel beside the viewer already shows the file's declarations, counts
    // and language -- so three tabs choosing between three directory analyses
    // are three tabs choosing nothing.
    useClient.setState((state) => ({ ...state, selected: "src/a.rs" }));
    render(<TopNav />);

    expect(control("Lines of code")).toBeNull();
    expect(control("Declarations")).toBeNull();
    expect(control("Dependencies")).toBeNull();
  });

  it("keeps the folder controls while a file is open", () => {
    // The opposite of the tabs: these apply whatever is selected, and hiding them
    // would leave no way to change what is being counted without closing the
    // file first -- which is the reason the navigation bar exists at all.
    useClient.setState((state) => ({ ...state, selected: "src/a.rs" }));
    render(<TopNav />);

    expect(control("Scan folder…")).toBeTruthy();
    expect(control("Count")).toBeTruthy();
    // The directory field, which is an input rather than a label.
    expect(screen.getByLabelText("Directory")).toBeTruthy();
    expect(screen.getByLabelText("Ignore patterns")).toBeTruthy();
  });

  it("brings the analyses back when the file is closed", async () => {
    useClient.setState((state) => ({ ...state, selected: "src/a.rs" }));
    render(<TopNav />);
    expect(control("Lines of code")).toBeNull();

    await act(async () => {
      useClient.setState((state) => ({ ...state, selected: null }));
    });
    expect(control("Lines of code")).toBeTruthy();
  });
});
