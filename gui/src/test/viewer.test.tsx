/**
 * The viewer's wiring to the highlighter, without importing Shiki.
 *
 * Shiki's own engine does not belong in a jsdom test, and the point of these is
 * not Shiki anyway: it is that the viewer *asks* for colour through the registry,
 * that a result is rendered as coloured spans, and that a count which disagrees
 * with the file's own lines falls back to plain text rather than mis-numbering
 * every line after it.
 *
 * The service is substituted through the registry, which is the seam the layering
 * exists to provide. `grammarFor` is the real one: whether a path *has* a grammar
 * is this application's decision and is not Shiki's.
 *
 * Token texts carry no leading space. `@testing-library` normalises whitespace
 * before matching, so a token that begins with one is invisible to `getByText`
 * even when it is in the document.
 */

import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { FileViewer } from "@/components/FileViewer";
import { useClient } from "@/state/store";
import { withServices } from "@/services/registry";
import type { ServiceRegistry } from "@/services/registry";
import type { Spans } from "@/lib/ipc";

/** A service whose grammar map says `rust` for one path and nothing for others. */
function withColours(path: string, spans: unknown): ReturnType<typeof vi.fn> {
  const tokenize = vi.fn().mockResolvedValue(spans);
  withServices({
    highlight: { tokenize, grammarFor: (p: string) => (p === path ? "rust" : null) },
  } as unknown as Partial<ServiceRegistry>);
  return tokenize;
}

/** Give the viewer one read of one file. */
function oneRead(text: string) {
  const readChunk = vi.fn().mockResolvedValue({
    text,
    bytes: text.length,
    total: text.length,
  });
  withServices({ viewer: { readChunk } } as unknown as Partial<ServiceRegistry>);
  return readChunk;
}

/** Seed a selected file and render. */
function show(path: string) {
  useClient.setState((state) => ({
    ...state,
    root: "/project",
    selected: path,
  }));
  return render(<FileViewer />);
}

/** The inline style of the token bearing `text`, as the browser holds it. */
function styleOf(text: string): string {
  return screen.getByText(text).getAttribute("style") ?? "";
}

/** One line, two tokens, a different colour in each theme. */
function oneColouredLine(): Spans {
  return [
    { text: "let", dark: "#569cd6", light: "#0000ff", italic: false, bold: false },
    { text: "s", dark: "#d4d4d4", light: "#000000", italic: false, bold: false },
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

describe("the viewer's highlighting", () => {
  it("asks for the file's grammar, by path", async () => {
    oneRead("fn f() {}\n");
    const tokenize = withColours("src/main.rs", null);

    await act(async () => {
      show("src/main.rs");
    });
    await act(async () => {});

    expect(tokenize).toHaveBeenCalledWith("fn f() {}\n", "rust");
  });

  it("does not ask for a file with no grammar", async () => {
    oneRead("body { color: red; }\n");
    const tokenize = vi.fn().mockResolvedValue(null);
    withServices({
      highlight: { tokenize, grammarFor: () => null },
    } as unknown as Partial<ServiceRegistry>);

    await act(async () => {
      show("src/theme.css");
    });
    await act(async () => {});

    expect(tokenize).not.toHaveBeenCalled();
  });

  it("paints in the dark theme's colour", async () => {
    oneRead("let s = 1;\n");
    withColours("src/a.rs", [oneColouredLine()]);

    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});

    // React writes `#569cd6` as `rgb(86, 156, 214)`, so the hex form is not what
    // the attribute holds.
    expect(styleOf("let")).toContain("color: rgb(86, 156, 214)");
    expect(styleOf("s")).toContain("color: rgb(212, 212, 212)");
  });

  it("paints in the light theme's colour when that is the theme", async () => {
    // Seeded through storage, because `useTheme` reads its first value from
    // there -- which is also why a reader's choice survives a restart.
    localStorage.setItem("sephera.theme", "light");
    oneRead("let s = 1;\n");
    withColours("src/a.rs", [oneColouredLine()]);

    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});

    expect(styleOf("let")).toContain("color: rgb(0, 0, 255)");
    expect(styleOf("let")).not.toContain("rgb(86, 156, 214)");
  });

  it("carries the grammar's italic and bold, and no colour when there is none", async () => {
    oneRead("fn f\n");
    withColours("src/a.rs", [
      [
        { text: "fn", dark: "#569cd6", light: "#0000ff", italic: false, bold: false },
        { text: "f", dark: null, light: null, italic: true, bold: true },
      ],
    ]);

    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});

    expect(styleOf("f")).toContain("font-style: italic");
    expect(styleOf("f")).toContain("font-weight: 600");
    // A token with no colour keeps no colour at all, rather than an empty one.
    expect(styleOf("f")).not.toContain("color:");
  });

  it("falls back to plain text when the spans do not match the lines", async () => {
    // One line of text, and two lines of spans back. The highlighter partitioned
    // the text differently, and rendering its spans anyway would put file content
    // under the wrong line number -- the one thing the numbers exist to prevent.
    oneRead("let s = 1;\n");
    withColours("src/a.rs", [
      [
        { text: "let", dark: "#569cd6", light: "#0000ff", italic: false, bold: false },
      ],
      [
        { text: "s = 1;", dark: "#d4d4d4", light: "#000000", italic: false, bold: false },
      ],
    ]);

    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});

    // The whole line as one uncoloured string, under number 1.
    const line = screen.getByText("1").nextElementSibling;
    expect(line?.textContent).toBe("let s = 1;");
    expect(line?.querySelector("span[style]")).toBeNull();
  });
});
