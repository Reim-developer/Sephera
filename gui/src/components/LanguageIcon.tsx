import {
  SiC,
  SiCplusplus,
  SiGo,
  SiJavascript,
  SiPython,
  SiRust,
  SiTypescript,
} from "react-icons/si";
import { DiJava } from "react-icons/di";
import type { IconType } from "react-icons";

/**
 * The icon for a language, the way a working tree names one.
 *
 * A table rather than a switch or a chain of conditionals: the map from a
 * language name to its icon is data, and it is the kind of data that grows by
 * one row when a language is added rather than by one branch. Every bundled
 * language has an entry, and a language without one falls back to a plain file
 * marker rather than being left blank -- a blank icon in a file tree reads as a
 * file the tool failed to recognise.
 *
 * The icons come from `react-icons`' Simple Icons set, which carries a
 * recognisable mark per language. That is the property that matters in a tree:
 * a reader scanning down a directory should not have to read the extension to
 * know what is what.
 */
const LANGUAGE_ICONS: Record<string, IconType> = {
  rust: SiRust,
  python: SiPython,
  typescript: SiTypescript,
  javascript: SiJavascript,
  go: SiGo,
  // Simple Icons carries no Java mark, so this comes from `di`, which is the
  // same package. Both sets ship in `react-icons`, so it is one dependency.
  java: DiJava,
  c: SiC,
  "c++": SiCplusplus,
  cpp: SiCplusplus,
};

/** What a language without a recognised icon renders as. */
const FALLBACK: IconType = SiC;

/**
 * The icon for a language name, as the report spells it.
 *
 * Matching is case-insensitive, because the report's names are capitalised
 * ("Rust") and the table's keys are not.
 */
export function languageIcon(language: string | null | undefined): IconType {
  if (!language) return FALLBACK;
  return LANGUAGE_ICONS[language.toLowerCase()] ?? FALLBACK;
}

/** Every language the icon table knows, for the fallback list in a test. */
export const KNOWN_LANGUAGES: readonly string[] = Object.keys(LANGUAGE_ICONS);
