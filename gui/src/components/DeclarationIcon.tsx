import { Braces, Box, Hash, Type } from "lucide-react";
import type { LucideIcon } from "lucide-react";

/**
 * The icon for a declaration kind, the way a working tree marks one.
 *
 * VS Code's outline view gives each kind its own mark, and the marks are worth
 * copying because they are chosen to be told apart at a glance: a function and
 * a type look nothing like each other, so a reader scanning a file's
 * declarations reads the shape rather than the word.
 *
 * The kinds are the analyzer's, which is `functions`, `types`, `enums`,
 * `constants` -- so the table is keyed by those rather than by the label the UI
 * shows, which is a second spelling of the same four things.
 */
const KIND_ICONS: Record<string, LucideIcon> = {
  // A brace marks a function: it opens a body.
  functions: Braces,
  // A type is a box, because it holds.
  types: Type,
  // An enum is a set: a brace is already a function, so a box with a mark.
  enums: Box,
  // A constant is a hash, which is the convention for "fixed value".
  constants: Hash,
};

/** The icon for a declaration kind, or a plain brace when unknown.
 *
 * A kind the table does not know falls back rather than rendering nothing: a
 * missing icon in a list of declarations reads as a row the tool failed on. */
export function declarationIcon(kind: string): LucideIcon {
  return KIND_ICONS[kind] ?? Braces;
}
