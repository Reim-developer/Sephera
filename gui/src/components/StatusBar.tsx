import { CircleAlert, FileClock, Settings } from "lucide-react";
import styles from "../styles/workbench.module.scss";

/**
 * The status bar: the number, and where it came from.
 *
 * The config path is shown because a count that changed because of a file the
 * user did not open is the single most confusing thing this tool can do. Saying
 * which `.sephera.toml` was read is the whole defence.
 */
export function StatusBar({
  summary,
  config,
  busy,
  onRecompute,
}: {
  summary: string | null;
  config: string | null;
  busy: boolean;
  onRecompute: () => void;
}) {
  return (
    <footer className={styles.workbench__status}>
      {busy ? (
        <span aria-live="polite">
          <CircleAlert size={12} aria-hidden="true" /> Working…
        </span>
      ) : (
        <span>{summary ?? "ready"}</span>
      )}
      {config ? (
        <span title={config}>
          <Settings size={12} aria-hidden="true" /> {config}
        </span>
      ) : null}
      {/*
        A recompute affordance on the status bar as well as in the controls. The
        keyboard shortcut is named where it can be seen, because a shortcut
        nobody is told about is a shortcut that does not exist.
      */}
      <span className={styles.kbd} onClick={onRecompute} role="presentation">
        <FileClock size={11} aria-hidden="true" /> Ctrl+R
      </span>
    </footer>
  );
}
