import { File, FileText } from "lucide-react";
import type { IconType } from "react-icons";

import { languageIcon } from "./LanguageIcon";

/**
 * The icon for a tree row: a directory's folder, or a file's language mark.
 *
 * A file's language is inferred from its extension, because that is what a
 * working tree shows and what a reader scanning a directory recognises. The
 * extension table is deliberately incomplete: an unknown extension falls back to
 * a plain file mark, which is the honest thing to show -- a file whose language
 * the table cannot name is not a file whose language should be invented.
 */
const BY_EXTENSION: Record<string, string> = {
  rs: "rust",
  py: "python",
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  go: "go",
  java: "java",
  c: "c",
  h: "c",
  cpp: "c++",
  cc: "c++",
  cxx: "c++",
  hpp: "c++",
  hh: "c++",
  hxx: "c++",
};

/**
 * The language a file path implies, or `null` when the extension is unknown.
 *
 * Only the final extension is consulted, because that is the one that names the
 * language: `mod.rs` is Rust and `index.d.ts` is TypeScript, and reading the
 * whole suffix as one string would match neither.
 */
export function languageOf(path: string): string | null {
  const name = path.split(/[\\/]/).pop() ?? path;
  const extension = name.includes(".") ? name.split(".").pop() : null;
  if (!extension) return null;
  return BY_EXTENSION[extension.toLowerCase()] ?? null;
}

/** The icon a file's path implies: its language mark, or a plain file. */
export function fileIcon(path: string): IconType {
  const language = languageOf(path);
  if (!language) return FileText;
  return languageIcon(language);
}

/** A directory's icon, shown for the row of a directory. */
export const DIRECTORY_ICON = File;
