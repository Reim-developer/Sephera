/**
 * The workbench's layout contract, read from the source.
 *
 * jsdom does not lay anything out, so a broken grid cannot be caught by rendering
 * one. What *can* be checked is the contract itself, which is a set of statements
 * in `workbench.module.scss` and `App.tsx` -- the same way `boundary.test.ts`
 * reads the Rust side to keep two spellings of one thing in step.
 *
 * The bug this pins was reported as "the file's content spills over the
 * declarations panel". The cause: a grid with rows but no columns has an implicit
 * `auto` column, and an `auto` track's growth limit is its items' max-content. A
 * file line thousands of characters wide therefore made the column that wide --
 * the box stayed at 100% while its column ran off the right edge and took the
 * panel with it. Every track now says `minmax(0, …)`, and the one component that
 * injects unbounded max-content (Radix's `display: table` content wrapper) is
 * gone from the content area.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const HERE = dirname(fileURLToPath(import.meta.url));
const GUI = resolve(HERE, "..");

/** The workbench stylesheet, as written. */
function scss(name: string): string {
  return readFileSync(resolve(GUI, "styles", name), "utf-8");
}

/** The block of one selector's declarations. */
function rule(sheet: string, selector: string): string {
  const at = sheet.indexOf(selector);
  if (at < 0) throw new Error(`no ${selector} in the sheet`);
  const open = sheet.indexOf("{", at);
  const close = sheet.indexOf("}", open);
  return sheet.slice(open + 1, close);
}

describe("the workbench grid", () => {
  it("declares a column that cannot exceed its container", () => {
    // `minmax(0, 1fr)` rather than the implicit `auto`. The second clause is what
    // keeps a long file line from setting the column's width.
    //
    // The declaration is matched, not the word: the block carries a comment
    // explaining the column, and a check that accepted prose would pass with the
    // declaration removed -- which is exactly what it is supposed to catch.
    const workbench = rule(scss("workbench.module.scss"), ".workbench {");
    expect(workbench).toMatch(/grid-template-columns:\s*minmax\(0,/);
  });

  it("declares one in the main column too, which was an implicit grid as well", () => {
    const main = rule(scss("workbench.module.scss"), ".workbench__main {");
    expect(main).toMatch(/grid-template-columns:\s*minmax\(0,/);
  });

  it("lets the split shrink", () => {
    // A flex item with no `min-width: 0` cannot shrink below its min-content, and
    // the split sits in a flex row beside the side panel.
    const split = rule(scss("workbench.module.scss"), ".split {");
    expect(split).toMatch(/min-width:\s*0/);
    expect(split).toMatch(/overflow:\s*hidden/);
  });

  it("makes the content area a real scroller", () => {
    const scroll = rule(scss("workbench.module.scss"), ".split__scroll {");
    expect(scroll).toMatch(/overflow:\s*auto/);
  });
});

describe("the content area", () => {
  it("does not wrap its content in a table", () => {
    // Radix's Viewport wraps children in `min-width: 100%; display: table`, and a
    // table sizes to its content. That wrapper is how a long line came to set the
    // width of a column three levels up, and the resizer's own dragging moved
    // that max-content around -- which is the symptom reported after a drag.
    // The import and the tag, not the mention: the file carries a comment saying
    // why Radix is gone, and a check that trips on its own explanation is a check
    // that will be deleted rather than corrected.
    const app = readFileSync(resolve(GUI, "App.tsx"), "utf-8");
    expect(app).not.toMatch(/from\s+"@radix-ui\/react-scroll-area"/);
    expect(app).not.toMatch(/<ScrollArea[\.\s]/);
  });
});
