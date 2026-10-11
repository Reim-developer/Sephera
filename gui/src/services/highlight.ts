/**
 * Layer 1: syntax highlighting, through Shiki.
 *
 * Shiki is the engine Visual Studio Code uses: TextMate grammars, so the colours
 * are the ones an editor shows rather than an independent set that happens to
 * resemble them. It costs nothing at start-up, because the highlighter is created
 * on the first file that needs it and each grammar is a dynamic import of its
 * own.
 *
 * Both themes are tokenised at once. That is the one decision worth explaining:
 * a theme switch that re-tokenised is a theme switch that hangs for half a second
 * on a large file, and `codeToTokensWithThemes` tokenises against a theme *map*
 * instead, so the switch is a lookup. The cost is a `variants` record per token
 * rather than a single colour, which is a byte or two.
 *
 * No WASM. Shiki v4 runs on a JavaScript regex engine, so there is nothing to
 * load and nothing that can fail to load -- which is the difference between a
 * highlighter and a build step.
 */


import type { Spans } from "@/lib/ipc";
import type { BundledLanguage, Highlighter } from "shiki";

/**
 * The grammar for a file, by extension.
 *
 * Keyed by extension rather than by the language name the report shows, because
 * the name is a display string -- `"C++"`, `"c++"` -- and a display string is a
 * thing that can be re-spelled. An extension is not. It is also the same table
 * the file tree already reads, so the two cannot disagree about which language a
 * `.cc` file is.
 *
 * The extensions are the ones `.sephera.toml`'s bundled languages declare, so a
 * grammar exists for every file the tool can analyse. A file with no grammar
 * simply shows uncoloured, which is the honest fallback: better plain source than
 * the wrong colours.
 */
const BY_EXTENSION: Record<string, string> = {
  rs: "rust",
  py: "python",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "tsx",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "jsx",
  go: "go",
  java: "java",
  // C and C++ share the `h` extension in C's own convention, and Shiki's C
  // grammar is the one that reads it. `hpp`, `hh` and `hxx` are unambiguously
  // C++'s.
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hh: "cpp",
  hxx: "cpp",
};

/** The themes tokenised against, named as the renderer reads them. */
const THEMES = { dark: "dark-plus", light: "light-plus" } as const;

/** The grammars requested on the first highlight, in map order. */
const GRAMMARS: readonly string[] = [...new Set(Object.values(BY_EXTENSION))];

/**
 * The Shiki grammar for a file, or `null` when there is none.
 *
 * Only the final extension is read, because that is the one that names the
 * language: `mod.rs` is Rust and `index.d.ts` is TypeScript, and reading the whole
 * suffix as one string would match neither.
 */
export function grammarFor(path: string): string | null {
  const name = path.split(/[\\/]/).pop() ?? path;
  const at = name.lastIndexOf(".");
  if (at < 0 || at === name.length - 1) return null;
  return BY_EXTENSION[name.slice(at + 1).toLowerCase()] ?? null;
}

/** The highlighter, created once and shared.
 *
 * A module-level promise rather than a field on the store, because it is a
 * capability the same way the directory dialog is one: it has no state a reader
 * would change, and the only caller cares whether it is ready.
 *
 * The JavaScript regex engine, chosen over Shiki's default. The default is
 * Oniguruma, which loads a **Wasm** binary before it will tokenise anything; a
 * webview that cannot fetch or compile it gets no colour, and the previous
 * version turned that into plain text with nothing said -- the reported failure
 * was a silent one. The JS engine implements the same interface with no Wasm
 * anywhere, and it reaches its first token sooner.
 *
 * A failed load resolves to `null` and is *reported*, not swallowed: the fallback
 * to plain text stays, because a highlighter that cannot load must not stop a
 * file being read, but an error nobody can see is an error nobody can fix. The
 * promise is left in place so the next file does not pay for the attempt again. */
let loading: Promise<Highlighter | null> | null = null;

async function highlighter(): Promise<Highlighter | null> {
  if (!loading) {
    loading = Promise.all([
      import("shiki"),
      import("shiki/engine/javascript"),
    ])
      .then(([{ createHighlighter }, { createJavaScriptRegexEngine }]) =>
        createHighlighter({
          engine: createJavaScriptRegexEngine(),
          langs: [...GRAMMARS],
          themes: [...new Set(Object.values(THEMES))],
        }),
      )
      .catch((error: unknown) => {
        console.error("sephera: the syntax highlighter could not load", error);
        return null;
      });
  }
  // A failed load resolves to `null` and leaves the promise in place, so the
  // next file does not pay for the attempt again -- but a transient failure of
  // the first import is not worth permanently disabling colour over, so the
  // promise is dropped and the next call retries.
  return loading;
}

/**
 * Tokenise `code` in `grammar`, in both themes.
 *
 * The last result is cached, and only the last. The viewer shows one file at a
 * time, so a one-entry cache covers every re-render -- including a theme switch,
 * which is the whole point of tokenising both themes at once -- and nothing else.
 * A larger cache would be memory held for a file nobody is reading.
 */
let cache: { key: string; spans: Spans[] } | null = null;

/** Tokenise, or `null` when the grammar or the highlighter is unavailable. */
export async function tokenize(
  code: string,
  grammar: string | null,
): Promise<Spans[] | null> {
  if (!grammar || !code) return null;

  const key = `${grammar}\u0000${code.length}\u0000${code}`;
  if (cache?.key === key) return cache.spans;

  const high = await highlighter();
  if (!high) return null;

  // `grammar` is a string, and shiki's overload wants one of the bundled
  // language names. The map's values *are* those names, so this narrows rather
  // than guesses -- and doing it here keeps the table itself free to name any
  // grammar without depending on shiki's union compiling first.
  const themed = high.codeToTokensWithThemes(code, {
    lang: grammar as BundledLanguage,
    themes: THEMES,
  });

  // The token's type is inferred from the call, so `variants` is reachable and
  // the grammar stays a `string`.
  const spans: Spans[] = themed.map((line) =>
    line.map((token) => {
      const dark = token.variants.dark;
      const light = token.variants.light;
      return {
        text: token.content,
        dark: dark?.color ?? null,
        light: light?.color ?? null,
        // Shiki's `fontStyle` is a bitmask: 1 for italic, 2 for bold.
        italic: Boolean(dark?.fontStyle && dark.fontStyle & 1),
        bold: Boolean(dark?.fontStyle && dark.fontStyle & 2),
      };
    }),
  );
  cache = { key, spans };
  return spans;
}

/** The service the registry holds, so a test can replace it. */
export const highlightService = {
  tokenize,
  grammarFor,
};
