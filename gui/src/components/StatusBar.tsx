import { useClient } from "@/state/store";
import styles from "@/styles/status.module.scss";

/** The status bar: where the pointer is, and what the file is written in.
 *
 * This is the bar Visual Studio Code keeps at the bottom of the editor, and it
 * earns its keep for the same reason: a reader following a declaration from the
 * panel beside the file wants to know the line they are looking at and the line
 * they are being pointed at. Three things, no more: the file's line-ending
 * sequence, its language, and the line and column under the pointer.
 *
 * It reads the store and renders. The viewer computes the caret, the host reports
 * the ending and the language, and nothing here knows either of them exists. */
export function StatusBar() {
  const caret = useClient((state) => state.caret);
  const selected = useClient((state) => state.selected);
  // The file's own report, which is what `file_detail` returned when it was
  // selected. Empty before a file is open, and the bar says so rather than
  // inventing values.
  const detail = useClient((state) => state.file.data);

  return (
    <footer className={styles.status} aria-label="File status">
      {selected ? (
        <span className={styles.status__item} title={selected}>
          {name(selected)}
        </span>
      ) : (
        <span className={styles.status__item}>No file open</span>
      )}

      <span className={styles.status__item}>{detail?.language ?? "—"}</span>

      <span className={styles.status__item} title="Line ending sequence">
        {detail?.eol ?? "—"}
      </span>

      <span className={styles.status__item}>
        Ln {caret.line}, Col {caret.column}
      </span>
    </footer>
  );
}

/** The final segment of a path. */
function name(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}
