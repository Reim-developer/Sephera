import { FileClock } from "lucide-react";
import { useClient } from "@/state/store";
import styles from "@/styles/workbench.module.scss";

/**
 * The status bar: the number, and where it came from.
 *
 * The config path is shown because a count that changed because of a file the
 * user did not open is the single most confusing thing this tool can do. Saying
 * which `.sephera.toml` was read is the whole defence.
 *
 * It calls commands rather than the store, so the recompute affordance here and
 * the one in the toolbar are the same code path.
 */
export function StatusBar({
  onRecompute,
  onPick,
}: {
  onRecompute: () => void;
  onPick: () => void;
}) {
  const summary = useClient((state) => {
    const loc = state.loc.data;
    if (!loc) return "ready";
    return `${loc.files_scanned.toLocaleString()} files, ${loc.rows.length} languages, ${loc.elapsed_ms} ms`;
  });
  const config = useClient((state) => state.loc.data?.config_source ?? null);
  const busy = useClient((state) => state.loc.busy || state.symbols.busy || state.graph.busy);

  return (
    <footer className={styles.workbench__status}>
      {busy ? <span aria-live="polite">Working…</span> : <span>{summary}</span>}
      {config ? <span title={config}>config: {config}</span> : null}
      <span className={styles.kbd} onClick={onRecompute} role="presentation">
        <FileClock size={11} aria-hidden="true" /> Ctrl+R
      </span>
      <button type="button" className={styles.kbd} onClick={onPick}>
        Open…
      </button>
    </footer>
  );
}
