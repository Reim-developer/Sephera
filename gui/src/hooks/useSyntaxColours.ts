/**
 * Layer 4: the highlighted text, as a hook.
 *
 * Tokenising a 64 KiB window of code costs between one and five hundred
 * milliseconds, and that is not a number that belongs inside a render. So the
 * colour is computed after paint and kept in state: the file appears plain, and
 * the colours arrive a moment later. A reader who scrolls immediately sees text
 * rather than a frozen window.
 *
 * The work is cancelled by a flag rather than left to finish, because changing
 * file while a tokenise is in flight would otherwise let the old file's colours
 * land beside the new file's text.
 *
 * Both calls go through the registry, the same route every other hook takes. The
 * first version imported `tokenize` directly, which compiled and ran and could
 * not be substituted in a test -- the seam the service layer exists to provide,
 * bypassed by a direct import from the layer above it.
 */

import { useEffect, useState } from "react";

import { services } from "@/services/registry";
import type { Spans } from "@/lib/ipc";

/** The highlighted lines, or `null` while they are on their way.
 *
 * `null` is not a failure: it means "plain text for now". A file with no grammar
 * stays `null` forever, which is the same thing, and the caller does not need to
 * know the difference. */
export function useSyntaxColours(
  text: string,
  path: string | null,
): Spans[] | null {
  const grammar = path ? services.highlight.grammarFor(path) : null;
  const [spans, setSpans] = useState<Spans[] | null>(null);

  useEffect(() => {
    if (!grammar || !text) {
      setSpans(null);
      return;
    }
    let cancelled = false;

    void (async () => {
      const tokenised = await services.highlight.tokenize(text, grammar);
      if (!cancelled) setSpans(tokenised);
    })();

    return () => {
      cancelled = true;
    };
  }, [text, grammar]);

  return spans;
}
