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
      <div className={styles.viewer__scroll}>
        {shown.map((line, index) => (
          <div className={styles.viewer__line} key={index}>
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
 * The colours that describe the same lines `shown` numbers.
 *
 * Two partitions of one file, and they differ by one in the ordinary case. The
 * highlighter splits on `\n` and keeps the empty line a file's final newline
 * creates -- so `"fn f() {}\n"` is two arrays to it. This viewer's numbering drops
 * that empty, because an editor does not number it. Comparing one against the
 * other directly meant the guard was never satisfied for a file ending in a
 * newline, which is nearly every source file: the viewer fell back to plain text
 * every time, and no amount of correct highlighting would have shown it.
 *
 * So the trailing empty is dropped from the colour side, once, and only when the
 * text really does end on a newline. The count is then checked rather than
 * assumed: a highlighter that partitions the text some *genuinely* different way
 * still yields `null`, and the viewer shows plain text rather than putting file
 * content under the wrong line number.
 */
function alignSpans(
  text: string,
  colours: Spans[] | null,
  shown: readonly string[],
): Spans[] | null {
  if (colours === null || colours.length === 0) return null;
  const aligned = text.endsWith("\n") ? colours.slice(0, -1) : colours;
  return aligned.length === shown.length ? aligned : null;
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
