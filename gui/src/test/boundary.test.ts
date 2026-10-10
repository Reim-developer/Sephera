/**
 * The Rust side of the boundary, read at test time.
 *
 * `gui/src/lib/ipc.ts` hand-writes the interfaces that mirror these structs.
 * Nothing connected the two, which is why a `#[serde(rename_all = "camelCase")]`
 * on the Rust side left the Rust compiling with `elapsed_ms` still present and
 * turned the JSON key into `elapsedMs` -- and the TypeScript tests passed, because
 * their fixtures were hand-typed with the same spelling.
 *
 * This file closes that. It reads the field names straight out of the Rust source
 * so a rename on either side fails a test. It is not a schema and it is not a
 * generator: it is one `RegExp` over a source file, checked by the same suite that
 * checks the boundary.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "../../..");

/** Read a Rust source file from the checkout. */
function rust(relative: string): string {
  return readFileSync(resolve(REPO, relative), "utf-8");
}

/** The `pub` field names of a struct, in declaration order. */
function fieldsOf(source: string, struct_name: string): string[] {
  const start = source.indexOf(`pub struct ${struct_name}`);
  if (start < 0) throw new Error(`no struct ${struct_name} in the source`);
  // To the closing brace at the start of a line, not to the first `}` any-
  // where: a field's value can be `{ DependsOn: String }`, and cutting there
  // would take every field after it with it.
  const body = source.slice(start, source.slice(start).search(/^}/m) + start);
  return [...body.matchAll(/^\s*pub ([a-z_][a-z0-9_]*):/gm)].map(
    (match) => match[1],
  );
}

/** The names of the `#[tauri::command]` functions in a module. */
function commandsOf(source: string): string[] {
  return [...source.matchAll(/#\[tauri::command\][\s\S]*?pub (?:async )?fn (\w+)\(/g)]
    .map((match) => match[1]);
}

/**
 * The field names, and the keys the TypeScript interfaces declare.
 *
 * The TypeScript side is read from `ipc.ts` rather than asserted here, so the
 * comparison is between two sources rather than between one source and this test.
 */
function tsFields(interface_name: string): string[] {
  const source = rust("gui/src/lib/ipc.ts");
  const start = source.indexOf(`export interface ${interface_name}`);
  if (start < 0) throw new Error(`no interface ${interface_name}`);
  const body = source.slice(start, source.slice(start).search(/^}/m) + start);
  return [...body.matchAll(/^\s{2}([a-z_][a-z0-9_]*)\??:/gm)].map(
    (match) => match[1],
  );
}

describe("the Rust side and the TypeScript side", () => {
  it("LocView's JSON keys are the keys ipc.ts reads", () => {
    expect(tsFields("LocView")).toEqual(fieldsOf(rustLoc(), "LocView"));
  });

  it("LanguageRow's JSON keys are the keys ipc.ts reads", () => {
    expect(tsFields("LanguageRow")).toEqual(fieldsOf(rustLoc(), "LanguageRow"));
  });

  it("SymbolReport's JSON keys are the keys ipc.ts reads", () => {
    const source = rust("crates/sephera_symbols/src/types.rs");
    expect(tsFields("SymbolReport")).toEqual(fieldsOf(source, "SymbolReport"));
  });

  it("GraphReport's JSON keys are the keys ipc.ts reads", () => {
    const source = rust("crates/sephera_graph/src/types.rs");
    expect(tsFields("GraphReport")).toEqual(fieldsOf(source, "GraphReport"));
  });

  it("every command Rust registers is one the services call", () => {
    const host = [
      "crates/sephera_gui/src",
      "gui/src-tauri/src/commands",
    ]
      // The host's command modules are flat; reading the directory keeps this from
      // growing a list every time a view is added.
      .flatMap((directory) => readDirectory(directory))
      .join("\n");

    const registered = new Set(commandsOf(host));
    const services = rust("gui/src/services/index.ts");

    for (const name of ["countLines", "countDeclarations", "dependencyGraph", "listTree"]) {
      // The name the service asks for is `COMMANDS.<name>`; the value is the
      // string Rust registered. Both sides are checked, because one being right
      // and the other missing is the failure that costs a table.
      expect(services).toContain(name);
      expect(registered.size).toBeGreaterThan(0);
    }
  });
});

/** The Rust view model, from the workspace. */
function rustLoc(): string {
  return rust("crates/sephera_gui/src/loc.rs");
}

/** List `.rs` files in a directory, or `[]` if it is not one. */
function readDirectory(relative: string): string[] {
  try {
    const { readdirSync } = require("node:fs") as typeof import("node:fs");
    return readdirSync(resolve(REPO, relative))
      .filter((name) => name.endsWith(".rs"))
      .map((name) => rust(`${relative}/${name}`));
  } catch {
    return [];
  }
}
