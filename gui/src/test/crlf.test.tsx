/** A CRLF file, which the highlighter and the viewer read differently.

A Windows file's lines are `"code\r"` to the viewer and `"code"` to the
highlighter, which strips the `\r` as part of the line break. Both sides count the
same number of lines, so a count-based guard is satisfied and every line's colour
is rejected only once the content is compared -- or the ends are normalised on the
way in, which is what this test pins.

The fixture spells the lines without the `\r`, because that is what the highlighter
returns; the normalisation is what makes the viewer agree with it.
*/

import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { FileViewer } from "@/components/FileViewer";
import { useClient } from "@/state/store";
import { withServices } from "@/services/registry";
import type { ServiceRegistry } from "@/services/registry";
import type { Spans } from "@/lib/ipc";

/** One line of `fn <name>() {}`, as a highlighter that strips `\r` returns it. */
function fnLine(name: string): Spans {
  return [
    { text: "fn ", dark: "#569cd6", light: "#0000ff", italic: false, bold: false },
    { text: name, dark: "#dcdcaa", light: "#795e26", italic: false, bold: false },
    { text: "() {}", dark: "#d4d4d4", light: "#000000", italic: false, bold: false },
  ];
}

function show(path: string) {
  useClient.setState((state) => ({
    ...state,
    root: "/project",
    selected: path,
  }));
  return render(<FileViewer />);
}

beforeEach(() => {
  withServices({});
});

describe("a file with CRLF line endings", () => {
  it("is painted, because its endings are normalised on the way in", async () => {
    const readChunk = vi.fn().mockResolvedValue({
      text: "fn f() {}\r\nfn g() {}\r\n",
      bytes: 20,
      total: 20,
    });
    withServices({
      viewer: { readChunk },
      highlight: {
        tokenize: vi.fn().mockResolvedValue([fnLine("f"), fnLine("g"), []]),
        grammarFor: () => "rust",
      },
    } as unknown as Partial<ServiceRegistry>);

    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});

    expect(document.querySelectorAll("span[style]").length).toBeGreaterThan(0);
  });

  it("shows the text without the carriage returns", async () => {
    // The `\r` was never the reader's to see: it is a line ending, and this file's
    // line endings have been normalised.
    const readChunk = vi.fn().mockResolvedValue({
      text: "fn f() {}\r\n",
      bytes: 10,
      total: 10,
    });
    withServices({
      viewer: { readChunk },
      highlight: {
        tokenize: vi.fn().mockResolvedValue([fnLine("f"), []]),
        grammarFor: () => "rust",
      },
    } as unknown as Partial<ServiceRegistry>);

    await act(async () => {
      show("src/a.rs");
    });
    await act(async () => {});

    const line = document.querySelector('[class*="viewer__text"]');
    expect(line?.textContent).toBe("fn f() {}");
  });
});
