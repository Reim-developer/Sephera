/**
 * The viewer paints a file that ends in a newline.
 *
 * The reported bug -- highlighting never appearing -- was an off-by-one between the
 * highlighter and the viewer's own line count. Shiki splits on `\n` and keeps the
 * empty line a file's final newline creates; the viewer's numbering drops it. The
 * guard compared one against the other, found them unequal, and showed plain text
 * for nearly every source file, because nearly every source file ends in a
 * newline.
 *
 * So the fixture here is shaped the way the real highlighter shapes it: three
 * arrays for two lines of code plus the final newline. The last one is empty, which
 * is the line the viewer does not number.
 */
import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FileViewer } from "@/components/FileViewer";
import { useClient } from "@/state/store";
import { withServices } from "@/services/registry";
import type { ServiceRegistry } from "@/services/registry";
/** A grammar map that says `rust` for one path. */
function withColours(path: string, spans: unknown): ReturnType<typeof vi.fn> {
  const tokenize = vi.fn().mockResolvedValue(spans);
  withServices({
    highlight: { tokenize, grammarFor: (p: string) => (p === path ? "rust" : null) },
  } as unknown as Partial<ServiceRegistry>);
  return tokenize;
}
/** One read of a whole small file. */
function oneRead(text: string) {
  const readChunk = vi.fn().mockResolvedValue({
    text,
    bytes: text.length,
    total: text.length,
  });
  withServices({ viewer: { readChunk } } as unknown as Partial<ServiceRegistry>);
  return readChunk;
}
function show(path: string) {
  useClient.setState((state) => ({
    ...state,
    root: "/project",
    selected: path,
  }));
  return render(<FileViewer />);
}
/** The inline style of the token bearing `text`, as the browser holds it.
 *
 * The first of them: a file with two `fn` on it has two, and asking for one would
 * be asking a question with no answer. */
function styleOf(text: string): string {
  return screen.getAllByText(text)[0].getAttribute("style") ?? "";
}
/** Two tokens, a different colour in each theme. */
function token(dark: string, light: string) {
  return [
    { text: "fn", dark, light, italic: false, bold: false },
    { text: "f", dark: "#d4d4d4", light: "#000000", italic: false, bold: false },
  ];
}
beforeEach(() => {
  withServices({});
  localStorage.removeItem("sephera.theme");
  useClient.setState((state) => ({
    ...state,
    root: "",
    selected: null,
    panelWidth: 320,
  }));
});
describe("the viewer, painting a file that ends in a newline", () => {
  it("paints it", async () => {
    // Two lines of code and a final newline, which the highlighter returns as
    // *three* arrays: the second line, the empty line after the last newline, and
    // nothing else. The viewer numbers two.
    oneRead("fn f() {}\nfn g() {}\n");
    withColours("src/a.rs", [token("#569cd6", "#0000ff"), token("#569cd6", "#0000ff"), []]);
    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});
    expect(styleOf("fn")).toContain("color:");
    // Both lines are numbered, and the third is not: the file's final newline
    // ends line two, it does not begin line three.
    expect(screen.getByText("1")).toBeTruthy();
    expect(screen.getByText("2")).toBeTruthy();
    expect(screen.queryByText("3")).toBeNull();
  });
  it("still shows plain text when the highlighter really does disagree", async () => {
    // Three arrays for two lines and no trailing empty: the count is now
    // genuinely off, and the guard has to hold.
    oneRead("fn f() {}\nfn g() {}\n");
    withColours("src/a.rs", [token("#569cd6", "#0000ff"), token("#569cd6", "#0000ff")]);
    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});
    const line = screen.getByText("2").nextElementSibling;
    expect(line?.querySelector("span[style]")).toBeNull();
  });
  it("paints a file with no trailing newline too", async () => {
    // The other side of the same rule: no final newline, so the highlighter
    // agrees without anything being dropped.
    oneRead("fn f() {}\nfn g() {}");
    withColours("src/a.rs", [token("#569cd6", "#0000ff"), token("#569cd6", "#0000ff")]);
    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});
    expect(styleOf("fn")).toContain("color:");
  });
});
