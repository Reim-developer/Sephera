/**
 * The highlighter, run for real.
 *
 * Everything else in this suite substitutes the service. This file does not,
 * because the thing that has to be checked here is Shiki itself: that the
 * grammars load, that tokenising produces coloured spans in both themes, and that
 * none of it needs WASM. The last one is the failure that was reported -- colour
 * silently absent -- and it was silent precisely because the service swallowed a
 * failed `create` and fell back to plain text.
 *
 * The engine is asserted, not just used. Shiki's default `createHighlighter`
 * reaches for the Oniguruma WASM engine, which a Tauri webview may not be able to
 * compile; `shiki/engine/javascript` is the same API with no Wasm anywhere, and
 * it is the one this service must be using.
 */

import { describe, expect, it, vi } from "vitest";

import { grammarFor, tokenize } from "@/services/highlight";

/** A line of Rust, with a keyword and a type in it. */
const RUST = "fn count() -> usize { 1 }\n";

describe("the highlighter, against the real grammars", () => {
  it("tokenises a file into coloured spans, in both themes", async () => {
    const spans = await tokenize(RUST, "rust");

    expect(spans).not.toBeNull();
    const [first] = spans!;
    // The line is more than one span: `fn`, ` count`, and so on were each
    // recognised separately, or there would be nothing to colour.
    expect(first.length).toBeGreaterThan(1);
    // Every token's text, joined, is the line -- a highlighter that reorders or
    // drops text is worse than no highlighter.
    expect(first.map((span) => span.text).join("")).toBe(RUST.trimEnd());
    // Some span carries a colour from each theme. Not necessarily the same span:
    // a keyword is one colour in light and another in dark.
    expect(first.some((span) => span.dark !== null)).toBe(true);
    expect(first.some((span) => span.light !== null)).toBe(true);
  });

  it("gives the two themes different colours", async () => {
    const spans = await tokenize(RUST, "rust");
    // If both themes resolved to the same palette, the toggle would change
    // nothing but the chrome -- which is the bug this pins.
    const [first] = spans!;
    const dark = first.map((span) => span.dark).join("|");
    const light = first.map((span) => span.light).join("|");
    expect(light).not.toBe(dark);
  });

  it("colours every language Sephera can analyse", async () => {
    // One per grammar the map declares. A grammar that cannot load is a language
    // shown uncoloured, which is the failure that is hardest to notice.
    const grammars = new Set(
      ["a.rs", "a.py", "a.ts", "a.tsx", "a.js", "a.jsx", "a.go", "a.java", "a.c", "a.h", "a.cpp", "a.hpp", "a.cc"]
        .map((path) => grammarFor(path))
        .filter((value): value is string => value !== null),
    );
    expect(grammars.size).toBeGreaterThan(5);

    for (const grammar of grammars) {
      const spans = await tokenize("let value = 1;\n", grammar);
      expect(spans, grammar).not.toBeNull();
      expect(spans!.length, grammar).toBeGreaterThan(0);
    }
  });

  it("resolves no grammar for a language that has none", async () => {
    // And therefore no spans: plain text is the honest fallback, not a
    // tokeniser refusing to run.
    expect(grammarFor("tokens.scss")).toBeNull();
    expect(await tokenize("a {}\n", null)).toBeNull();
  });

  it("compiles no Wasm while it tokenises", async () => {
    // Shiki's default engine is Oniguruma, which compiles a Wasm binary before it
    // will colour anything. A webview that cannot fetch or compile that binary
    // gets no colour at all -- which is the failure that was reported, and it was
    // silent, so it is pinned here.
    //
    // Every function the global offers is recorded, not just `instantiate`: the
    // Oniguruma wrapper reaches for `new WebAssembly.Module(...)`, and an
    // assertion that spies on two names misses that call and passes against the
    // very bug it is meant to catch. Watching this fail: drop `engine:` from the
    // service's `createHighlighter` call, and this test fails on `Module`.
    const touched: string[] = [];
    const real = globalThis.WebAssembly;
    const wrapped = {} as Record<string, unknown>;
    for (const key of Object.keys(real) as (keyof typeof real)[]) {
      const value = real[key];
      if (typeof value !== "function") {
        wrapped[key as string] = value;
        continue;
      }
      wrapped[key as string] = (...args: unknown[]) => {
        touched.push(String(key));
        return (value as (...a: unknown[]) => unknown)(...args);
      };
    }

    vi.stubGlobal("WebAssembly", wrapped);
    try {
      await tokenize(RUST, "rust");
    } finally {
      vi.unstubAllGlobals();
    }

    expect(touched).toEqual([]);
  });
});
