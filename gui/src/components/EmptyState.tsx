import { FolderOpen } from "lucide-react";

import { run } from "@/platform/commands";
import { commandIds } from "@/platform/commands";
import styles from "@/styles/empty.module.scss";

/**
 * What the workbench shows before a directory is chosen.
 *
 * The tables are the product, and a table of numbers about nowhere is worse
 * than a prompt naming what is missing. So there is one action, and it is the
 * one the whole application turns on.
 *
 * The store's root starts empty rather than `"."` because a relative default
 * resolves against the host process's working directory -- which under
 * `tauri dev` is `gui/src-tauri`, so `.` counted that. An empty root means no
 * analysis rather than a wrong one.
 */
export function EmptyState() {
  return (
    <div className={styles.empty}>
      <FolderOpen size={48} strokeWidth={1} aria-hidden="true" />
      <h1 className={styles.empty__title}>Open a directory</h1>
      <p className={styles.empty__hint}>
        Choose a folder to count its lines of code, declarations, and
        dependencies.
      </p>
      <button
        type="button"
        className={styles.empty__button}
        onClick={() => void run(commandIds.pickDirectory)}
      >
        Open…
      </button>
      <p className={styles.empty__shortcut}>
        or press <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>O</kbd>
      </p>
    </div>
  );
}
