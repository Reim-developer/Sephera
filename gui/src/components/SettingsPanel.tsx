import { FileText } from "lucide-react";

import { useClient } from "@/state/store";
import styles from "@/styles/settings.module.scss";

/**
 * The settings panel: what was read, and what it was read with.
 *
 * This carries the two things the status bar used to, because deleting that bar
 * would otherwise lose them. The config source is the more important of the two:
 * a count that moved because of a file the reader never opened is the single
 * most confusing thing this tool can do, and saying which `.sephera.toml` was
 * read is the whole defence.
 *
 * The analysis controls are *not* here. They live in the top navigation, where
 * they stay visible while a file is open -- which was the reason for the
 * navigation bar in the first place.
 */
export function SettingsPanel() {
  const loc = useClient((state) => state.loc.data);
  const symbols = useClient((state) => state.symbols.data);
  const ignore = useClient((state) => state.ignore);
  const root = useClient((state) => state.root);

  return (
    <div className={styles.settings}>
      <header className={styles.settings__header}>
        <h2 className={styles.settings__title}>Settings</h2>
        <p className={styles.settings__path} title={root}>
          {root || "No folder scanned yet"}
        </p>
      </header>

      <dl className={styles.settings__list}>
        <Row
          label="Configuration file"
          value={loc?.config_source ?? "none read"}
          hint={
            loc
              ? "The .sephera.toml whose rules produced these numbers."
              : "Scan a folder to see which config it reads."
          }
        />
        <Row
          label="Ignore patterns"
          value={
            ignore.length > 0 ? ignore.join(", ") : "none added"
          }
          hint="Merged after the config file's own patterns."
        />
        <Row
          label="Files scanned"
          value={loc ? loc.files_scanned.toLocaleString() : "—"}
          hint="Directories walked for the line count."
        />
        <Row
          label="Languages"
          value={loc ? loc.rows.length.toLocaleString() : "—"}
        />
        <Row
          label="Declared symbols"
          value={symbols ? symbolTotal(symbols.totals).toLocaleString() : "—"}
          hint={
            symbols
              ? `${symbols.files_skipped.toLocaleString()} file(s) skipped, having no bundled language.`
              : undefined
          }
        />
        <Row
          label="Last scan"
          value={loc ? `${loc.elapsed_ms.toLocaleString()} ms` : "—"}
        />
      </dl>

      {ignore.length > 0 ? (
        <p className={styles.settings__note}>
          <FileText size={12} aria-hidden="true" />
          A pattern added here is merged after the config file&rsquo;s, so it
          cannot remove a rule the file sets.
        </p>
      ) : null}
    </div>
  );
}

/** One labelled figure. */
function Row({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className={styles.settings__row}>
      <dt className={styles.settings__label}>
        {hint ? (
          <span title={hint} aria-label={label}>
            {label}
          </span>
        ) : (
          label
        )}
      </dt>
      <dd className={styles.settings__value}>{value}</dd>
    </div>
  );
}

/** Every declared symbol, across the report's per-kind totals. */
function symbolTotal(totals: Record<string, number | undefined>): number {
  let sum = 0;
  for (const count of Object.values(totals)) sum += count ?? 0;
  return sum;
}
