import {
  FolderSearch,
  Moon,
  PanelLeft,
  RefreshCw,
  Settings,
  Sun,
} from "lucide-react";

import { commandIds, run } from "@/platform/commands";
import { useTheme } from "@/hooks/useTheme";
import { useClient } from "@/state/store";
import { VIEWS } from "@/views/registry";
import { TabBar } from "@/components/TabBar";
import styles from "@/styles/nav.module.scss";

/**
 * The top navigation: two rows, and always visible.
 *
 * Row one is the analysis itself -- which folder, which patterns, and the button
 * that recounts. Row two is which analysis. Both stay put when a file is open,
 * and that is the whole reason the bar exists: the controls used to live in the
 * content area, so opening a file replaced them with the file's text and there
 * was no way to change what was being counted without closing it first.
 *
 * The old left rail is gone. Its "Configuration" button opened nothing, and a
 * panel that opens nothing is worse than a missing one -- so what it was for
 * lives here instead, in the side panel `Settings` opens.
 *
 * Nothing here owns state. The fields hold a draft of their own because a
 * controlled input bound to the store would recount on every keystroke, and the
 * panel toggles and tabs read and write the store directly.
 */
export function TopNav() {
  const root = useClient((state) => state.root);
  const view = useClient((state) => state.view);
  const panel = useClient((state) => state.panel);
  const togglePanel = useClient((state) => state.togglePanel);
  const setView = useClient((state) => state.setView);
  // A file being read replaces all three directory analyses, so the tab row that
  // chooses between them has nothing to choose. The controls above it stay: the
  // folder and the patterns apply whatever is selected.
  const selected = useClient((state) => state.selected);

  const { theme, toggle } = useTheme();

  return (
    <header className={styles.nav}>
      <div className={styles.nav__row}>
        <div className={styles.nav__group}>
          <span className={styles.nav__brand}>Sephera</span>

          <SidePanelToggle
            icon={PanelLeft}
            label="Explorer"
            open={panel === "explorer"}
            onToggle={() => togglePanel("explorer")}
          />
          <SidePanelToggle
            icon={Settings}
            label="Settings"
            open={panel === "settings"}
            onToggle={() => togglePanel("settings")}
          />

          <button
            type="button"
            className={styles.nav__iconButton}
            aria-label={theme === "dark" ? "Switch to the light theme" : "Switch to the dark theme"}
            title={theme === "dark" ? "Switch to the light theme" : "Switch to the dark theme"}
            onClick={toggle}
          >
            {theme === "dark" ? (
              <Sun size={16} strokeWidth={1.5} aria-hidden="true" />
            ) : (
              <Moon size={16} strokeWidth={1.5} aria-hidden="true" />
            )}
          </button>
        </div>

        <div className={styles.nav__group}>
          <button
            type="button"
            className={styles.nav__button}
            onClick={() => void run(commandIds.pickDirectory)}
            title="Choose a folder to scan (Ctrl+Shift+O)"
          >
            <FolderSearch size={13} aria-hidden="true" />
            Scan folder…
          </button>

          <DirectoryField root={root} />
          <IgnoreField />

          <button
            type="button"
            className={styles.nav__button}
            onClick={() => void run(commandIds.recompute)}
            title="Run the showing analysis again (F5 or Ctrl+R)"
          >
            <RefreshCw size={13} aria-hidden="true" />
            Count
          </button>
        </div>
      </div>

      {/* No root means no analysis to switch between, so the tab row appears with
          the first scan rather than sitting there disabled. */}
      {/* The tab row appears once there is something to switch between, and hides
          again while a file is open. The three tabs choose between three
          *directory* analyses, and the viewer has replaced all three; the
          controls above stay, because the folder and the patterns apply whatever
          is selected. Clearing the selection brings the row back, which is also
          how a reader returns to those views. */}
      {root && !selected ? (
        <TabBar
          tabs={VIEWS.map((entry) => ({ id: entry.id, label: entry.label }))}
          active={view}
          onSelect={setView}
        />
      ) : null}
    </header>
  );
}

/** The directory field: local state for the keystrokes, committed on Enter.
 *
 * A controlled input bound straight to the store would recount every analysis on
 * each character, so the field keeps its own draft and hands it over on Enter.
 * That is the one place a component holds state, and it is transient by design:
 * nothing else reads it. */
function DirectoryField({ root }: { root: string }) {
  const setRoot = useClient((state) => state.setRoot);

  return (
    <input
      className={styles.nav__field}
      defaultValue={root}
      key={root}
      aria-label="Directory"
      placeholder="/path/to/project"
      title="The folder being analysed. Committed on Enter."
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          const value = (event.target as HTMLInputElement).value.trim();
          if (value && value !== root) setRoot(value);
        }
      }}
    />
  );
}

/** The ignore-patterns field, committed on Enter for the same reason. */
function IgnoreField() {
  const ignore = useClient((state) => state.ignore);
  const setIgnore = useClient((state) => state.setIgnore);

  return (
    <input
      className={styles.nav__field}
      defaultValue={ignore.join(", ")}
      key={ignore.join(",")}
      aria-label="Ignore patterns"
      placeholder="ignore: target, *.snap"
      title="Comma-separated. Committed on Enter."
      onKeyDown={(event) => {
        if (event.key !== "Enter") return;
        const value = (event.target as HTMLInputElement).value;
        setIgnore(
          value
            .split(",")
            .map((entry) => entry.trim())
            .filter((entry) => entry.length > 0),
        );
      }}
    />
  );
}

/**
 * A side-panel toggle.
 *
 * Pressed rather than highlighted, because it is a switch: the same click opens
 * and closes, and a reader should be able to see which state they are in without
 * looking at the sidebar.
 */
function SidePanelToggle({
  icon: Icon,
  label,
  open,
  onToggle,
}: {
  icon: typeof PanelLeft;
  label: string;
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      className={styles.nav__iconButton}
      aria-pressed={open}
      aria-label={label}
      title={`${label} panel`}
      onClick={onToggle}
    >
      <Icon size={16} strokeWidth={1.5} aria-hidden="true" />
    </button>
  );
}
