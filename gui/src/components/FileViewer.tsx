import { useEffect, useRef, useState } from "react";
import { AlertTriangle } from "lucide-react";

import { readFileChunk } from "@/lib/tauri";
import { useClient } from "@/state/store";
import styles from "@/styles/viewer.module.scss";

/** How many bytes the first read takes, and what "load more" adds.
 *
 * A source file is a few kilobytes and a log can be a gigabyte, so the first
 * read is deliberately small: the panel opens with something readable in it
 * rather than stalling on a file that may never be read to the end.
 */
const INITIAL_BYTES = 64 * 1024;
const STEP_BYTES = 64 * 1024;

/** How far a chunk may be split across a UTF-8 boundary. */
const MAX_CHUNK = STEP_BYTES;

/**
 * The selected file's text, read in chunks.
 *
 * This is what the workbench shows when a single file is selected and nothing
 * else: a file's contents, not a table about it. The three analyses answer
 * questions about a *directory*; once the reader has narrowed to one file the
 * question is simply what it says.
 *
 * The read goes through the host, which owns the filesystem, rather than
 * through `fetch("file://")` -- the webview's file access is its own policy
 * and a command is what the rest of the client already talks to.
 */
export function FileViewer() {
  const selected = useClient((state) => state.selected);
  const root = useClient((state) => state.root);
  const [text, setText] = useState("");
  const [loaded, setLoaded] = useState(0);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);

  // A new file resets the read. The dependency is the selection, and a stale
  // chunk from the previous file would otherwise sit above the new one.
  useEffect(() => {
    if (!selected) return;
    let cancelled = false;
    setText("");
    setLoaded(0);
    setError(null);

    void (async () => {
      try {
        const start = await readFileChunk(root, selected, 0, INITIAL_BYTES);
        if (cancelled) return;
        setText(start.text);
        setLoaded(start.bytes);
        setTotal(start.total);
      } catch (cause) {
        if (!cancelled) setError(String(cause));
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [root, selected]);

  if (!selected) return null;

  if (error) {
    return (
      <div className={styles.viewer__error}>
        <AlertTriangle size={16} aria-hidden="true" />
        <span>Cannot read {selected}: {error}</span>
      </div>
    );
  }

  const hasMore = loaded < total;

  return (
    <div className={styles.viewer} ref={scroller}>
      <pre className={styles.viewer__pre}>{text}</pre>
      {hasMore ? (
        <button
          type="button"
          className={styles.viewer__more}
          onClick={() => {
            void (async () => {
              const step = Math.min(total - loaded, MAX_CHUNK);
              const more = await readFileChunk(root, selected, loaded, step);
              setText((previous) => previous + more.text);
              setLoaded(more.bytes);
            })();
          }}
        >
          Load {(Math.min(total - loaded, MAX_CHUNK) / 1024).toFixed(0)} KiB more
          ({(loaded / 1024).toFixed(0)} of {(total / 1024).toFixed(0)} KiB read)
        </button>
      ) : (
        <p className={styles.viewer__end}>End of file</p>
      )}
    </div>
  );
}
