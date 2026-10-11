/**
 * The grammar map, and what a file of each language resolves to.
 *
 * These drive at the map rather than through Shiki, because the map is the part
 * that is this application's: whether a `.cc` file gets C++'s colours is a
 * decision made here and tested here, and Shiki's own behaviour is its own
 * problem.
 *
 * The last case is the one that matters most. A file whose extension has no
 * grammar resolves to `null`, and the viewer then shows plain source -- which is
 * the honest fallback. A map that guessed at an unknown extension would give a
 * `.scss` file Rust's colours and nobody would know why the colours were wrong.
 */

import { describe, expect, it } from "vitest";

import { grammarFor } from "@/services/highlight";

describe("the grammar map", () => {
  it("resolves every language the tool can analyse", () => {
    // One per language, taken from the extensions `.sephera.toml`'s bundled
    // languages declare. A grammar exists for each, which is the point: there is
    // no file Sephera counts that has to fall back to plain text.
    const cases: Record<string, string> = {
      "src/main.rs": "rust",
      "src/lib.py": "python",
      "src/app.ts": "typescript",
      "src/app.tsx": "tsx",
      "src/app.js": "javascript",
      "src/app.jsx": "jsx",
      "main.go": "go",
      "Main.java": "java",
      "k.c": "c",
      "k.h": "c",
      "w.cpp": "cpp",
      "w.cc": "cpp",
      "w.hpp": "cpp",
      "w.hh": "cpp",
      "w.hxx": "cpp",
    };

    for (const [path, expected] of Object.entries(cases)) {
      expect(grammarFor(path), path).toBe(expected);
    }
  });

  it("reads the final extension, not the whole suffix", () => {
    // `index.d.ts` is TypeScript's declaration file, and reading the suffix as
    // one string would match neither `ts` nor any language at all.
    expect(grammarFor("src/index.d.ts")).toBe("typescript");
    expect(grammarFor("src/mod.rs")).toBe("rust");
  });

  it("resolves nothing for an extension it does not know", () => {
    // The failures are paths that are not source. None of them may be given a
    // grammar it does not have.
    for (const path of [
      "tokens.scss",
      "theme.css",
      "Cargo.toml",
      "README.md",
      "Makefile",
      "notes",
      ".gitignore",
      "src",
      "file.",
    ]) {
      expect(grammarFor(path), path).toBeNull();
    }
  });

  it("resolves nothing for an empty path", () => {
    expect(grammarFor("")).toBeNull();
  });

  it("reads past a directory's dots", () => {
    // A directory with a dot in it is not a language.
    expect(grammarFor("my.project/main.go")).toBe("go");
    expect(grammarFor("a.b/c.rs")).toBe("rust");
  });
});
