import type { LucideIcon } from "lucide-react";
import {
  FileCode2,
  Hash,
  Settings2,
  TextSearch,
} from "lucide-react";

import styles from "@/styles/workbench.module.scss";

/** One panel the activity bar can open. */
export interface ActivityItem {
  id: string;
  label: string;
  /** A real icon set rather than a glyph, so the rail weights consistently. */
  icon: LucideIcon;
}

/**
 * The activity bar's panels.
 *
 * `search` is absent on purpose: it is a panel the CLI does not have a
 * corresponding answer for, and an icon that opens nothing is worse than a
 * missing one.
 */
export const PANELS: readonly ActivityItem[] = [
  { id: "explorer", label: "Explorer", icon: FileCode2 },
  { id: "config", label: "Configuration", icon: Settings2 },
];

/**
 * The left rail that chooses which sidebar is open.
 *
 * One selected item at a time, and clicking the selected one closes the panel.
 * With a sidebar this narrow, hiding it is how a reader reclaims the width, and
 * a second click being a no-op is what makes the rail feel stuck.
 */
export function ActivityBar({
  items,
  active,
  onSelect,
}: {
  items: readonly ActivityItem[];
  active: string | null;
  onSelect: (id: string | null) => void;
}) {
  return (
    <nav className={styles.activityBar} aria-label="Views">
      {items.map(({ id, label, icon: Icon }) => {
        const selected = active === id;
        return (
          <button
            key={id}
            type="button"
            className={styles.activityBar__item}
            aria-pressed={selected}
            aria-label={label}
            title={label}
            onClick={() => onSelect(selected ? null : id)}
          >
            <Icon size={22} strokeWidth={1.5} aria-hidden="true" />
          </button>
        );
      })}
    </nav>
  );
}

/** The search panel's icon, kept out of `PANELS` because there is no panel. */
export const SEARCH_ICON: LucideIcon = TextSearch;
export const SYMBOL_ICON: LucideIcon = Hash;
