import { AlertTriangle } from "lucide-react";

import { useFileViewer } from "@/hooks/useFileViewer";
import { useSyntaxColours } from "@/hooks/useSyntaxColours";
import { useTheme } from "@/hooks/useTheme";
import { useClient } from "@/state/store";
import type { ThemeId } from "@/lib/ipc";
import type { Spans } from "@/lib/ipc";
import styles from "@/styles/viewer.module.scss";

/** How many bytes one read takes, and what "load more" adds.
 *
 * Kept here rather than in the hook because it is a display decision: how much
 * text the panel shows before asking, and how much it adds when it asks. The
 * hook's job is where the offset is, not how big it should be. */
const CHUNK_BYTES = 64 * 1024;

/**
 * The selected file's text, one line at a time, numbered.
 *
 * This is what the workbench shows when a single file is selected and nothing
 * else: a file's contents, not a table about it. The three analyses answer
 * questions about a *directory*; once the reader has narrowed to one file the
 * question is simply what it says.
 *
 * Rendering only. The reading, the offset and the failure live in
 * `useFileViewer`, and the colour in `useSyntaxColours`, because a component that
 * fetches has no seam a test can substitute -- which is the whole reason `hooks/`
 * exists as a layer.
 *
 * The text is held whole and split at render, rather than accumulated as an array
 * of lines. That is the only way a chunk boundary is invisible: two chunks joined
 * as strings preserve every newline exactly, where joining them as arrays needs a
 * rule for the seam, and a rule for the seam is a rule that can be wrong.
 */
export function FileViewer() {
  const { text, loaded, total, error, loadMore } = useFileViewer();
  const selected = useClient((state) => state.selected);
  const colours = useSyntaxColours(text, selected);
  const { theme } = useTheme();
  const setCaret = useClient((state) => state.setCaret);

  if (!selected) return null;

  if (error) {
    return (
      <div className={styles.viewer__error}>
        <AlertTriangle size={16} aria-hidden="true" />
        <span>Cannot read {selected}: {error}</span>
      </div>
    );
  }

  if (!text) return null;

  const hasMore = loaded < total;

  const shown = lines(text);
  const painted = alignSpans(text, colours, shown);

  return (
    <div className={styles.viewer}>
      <div
        className={styles.viewer__scroll}
        onMouseMove={(event) => onMove(event, setCaret)}
        onMouseLeave={() => setCaret({ line: 1, column: 1 })}
      >
        {shown.map((line, index) => (
          <div
            className={styles.viewer__line}
            key={index}
            data-line={index + 1}
          >
            <span className={styles.viewer__number} aria-hidden="true">
              {index + 1}
            </span>
            <span className={styles.viewer__text}>
              {painted ? <Painted spans={painted[index]} theme={theme} /> : line}
            </span>
          </div>
        ))}
      </div>

      {hasMore ? (
        <button
          type="button"
          className={styles.viewer__more}
          onClick={loadMore}
        >
          Load {(Math.min(total - loaded, CHUNK_BYTES) / 1024).toFixed(0)} KiB
          more ({(loaded / 1024).toFixed(0)} of{" "}
          {(total / 1024).toFixed(0)} KiB read)
        </button>
      ) : null}
    </div>
  );
}

/** One line, painted.
 *
 * Each token carries both themes' colours and the active one is picked here, so a
 * theme switch is a re-render rather than a re-tokenise. A token the grammar left
 * uncoloured keeps the default, because the file's own colour is the one the
 * viewer already shows and inventing one would be worse. */
function Painted({
  spans,
  theme,
}: {
  spans: Spans;
  theme: ThemeId;
}) {
  return (
    <>
      {spans.map((span, index) => (
        <span
          key={index}
          style={{
            color: (theme === "light" ? span.light : span.dark) ?? undefined,
            fontStyle: span.italic ? "italic" : undefined,
            fontWeight: span.bold ? 600 : undefined,
          }}
        >
          {span.text}
        </span>
      ))}
    </>
  );
}

/**
 * The colours that describe the same lines `shown` numbers, or `null` if none do.
 *
 * Three partitions of one file have to agree before any of it is safe to draw, and
 * the first two are the ones that cost a real bug:
 *
 * - The highlighter splits on `\n` and *keeps* the empty line a file's final
 *   newline creates, so `"fn f() {}\n"` is two arrays to it. The viewer's
 *   numbering drops that empty, because an editor does not number it. Comparing the
 *   two directly meant this never matched for a file ending in a newline -- nearly
 *   every source file -- and no amount of correct highlighting would have shown.
 *   Hence the trim.
 * - A file with `\r\n` endings has its `\r` dropped by the highlighter, which
 *   strips it as part of the line break. Normalising those away on the way in is
 *   what makes the two sides describe the same text.
 * - And a count that agrees is not a partition that agrees: two sequences of equal
 *   length can still be offset by a line, and a one-line offset is exactly the
 *   reported symptom -- a comment's colour sitting on the code beneath it. So each
 *   line's text is checked, not just how many lines there are.
 *
 * The fallback is plain text, and it is reported once. A silent fallback is a
 * fallback nobody can fix, and this one exists because a failure here is invisible
 * until somebody reads a file and sees the wrong colour.
 */
function alignSpans(
  text: string,
  colours: Spans[] | null,
  shown: readonly string[],
): Spans[] | null {
  if (colours === null || colours.length === 0) return null;

  const aligned = text.endsWith("\n") ? colours.slice(0, -1) : colours;
  if (aligned.length !== shown.length) return null;

  for (let index = 0; index < aligned.length; index += 1) {
    const joined = aligned[index].map((span) => span.text).join("");
    if (joined !== shown[index]) {
      console.warn(
        `sephera: the highlight for line ${index + 1} does not match its text; ` +
          "showing the file without colour.",
      );
      return null;
    }
  }
  return aligned;
}

/** Where the pointer is, as a line and a column.
 *
 * The row under the pointer gives the line; `caretRangeFromPoint` gives the offset
 * into it, and the offset is the column. Both APIs are Chromium/WebKit's, which is
 * what Tauri ships on Windows and Linux -- the one place this runs.
 *
 * A row that is empty still reports a column, because the pointer landed
 * somewhere: column 1 is the only honest answer for a line with nothing in it. */
function onMove(
  event: React.MouseEvent<HTMLDivElement>,
  setCaret: (caret: { line: number; column: number }) => void,
): void {
  const row = (event.target as HTMLElement).closest("[data-line]");
  if (!(row instanceof HTMLElement)) return;

  const line = Number(row.dataset.line);
  const text = row.querySelector("[class*=viewer__text]");
  const column = columnAt(text, event.clientX, event.clientY);
  if (Number.isFinite(line) && column > 0) setCaret({ line, column });
}

/** The 1-based column at the pointer's position inside one line's text.
 *
 * Both coordinates, not just x: `caretRangeFromPoint` needs the point, and a y of
 * 0 asks about the top of the document, which is some other line entirely. */
function columnAt(
  box: Element | null,
  clientX: number,
  clientY: number,
): number {
  if (!box) return 0;
  const range = document.caretRangeFromPoint(clientX, clientY);
  if (!range) return 0;

  // The offset is into whatever text node the point landed in, which for a
  // highlighted line is one token span's text. Walking the line's text nodes and
  // summing the ones before it is what turns that into a column.
  let column = 0;
  const walker = document.createTreeWalker(box, NodeFilter.SHOW_TEXT);
  let node = walker.nextNode();
  while (node) {
    if (node === range.startContainer) {
      return column + range.startOffset + 1;
    }
    column += node.textContent?.length ?? 0;
    node = walker.nextNode();
  }
  return column + 1;
}

/**
 * The file's lines, in order.
 *
 * The trailing empty string of a split is not a line: a file ending in a newline
 * ends *on* that newline, and the editor that wrote it would say it has one line
 * rather than two. Dropping exactly one is the rule, because `"a\n\n"` really
 * does have an empty second line.
 */
function lines(text: string): string[] {
  const split = text.split("\n");
  if (split.length > 1 && split[split.length - 1] === "") split.pop();
  return split;
}
