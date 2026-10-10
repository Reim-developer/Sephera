import { useEffect } from "react";
import * as ScrollArea from "@radix-ui/react-scroll-area";
import { FolderOpen, RefreshCw } from "lucide-react";

import { ActivityBar, PANELS } from "@/components/ActivityBar";
import { useActions } from "@/hooks/useActions";
import { FileTree } from "@/components/FileTree";
import { StatusBar } from "@/components/StatusBar";
import { TabBar } from "@/components/TabBar";
import { useGeneration } from "@/hooks/useAnalysis";
import { commandIds, run } from "@/platform/commands";
import { useClient } from "@/state/store";
import styles from "@/styles/workbench.module.scss";
import editor from "@/styles/editor.module.scss";
import { VIEWS } from "@/views/registry";

export function App() {
  const root = useClient((state) => state.root);
  const selected = useClient((state) => state.selected);
  const view = useClient((state) => state.view);
  const generation = useGeneration();
  const { setSelected, setView } = useActions();

  // ---- keyboard ---------------------------------------------------------
  // Both shortcuts run commands rather than calling the store, so a keybinding
  // and a button are one code path. `preventDefault` matters: a browser's F5
  // reloads the document, and in a Tauri app there is no document to reload.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "F5" || (event.ctrlKey && event.key === "r")) {
        event.preventDefault();
        void run(commandIds.recompute);
      }
      if (event.ctrlKey && event.shiftKey && event.key === "O") {
        event.preventDefault();
        void run(commandIds.pickDirectory);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const active = VIEWS.find((entry) => entry.id === view) ?? VIEWS[0];
  const Active = active.component;

  return (
    <div className={styles.workbench}>
      <ActivityBar items={PANELS} active="explorer" onSelect={() => undefined} />

      <aside className={styles.sidebar}>
        <h2 className={styles.sidebar__title}>
          <FolderOpen size={12} aria-hidden="true" /> Explorer
        </h2>
        <div className={styles.sidebar__body}>
          <FileTree
            root={root}
            selected={selected}
            // The store sets the graph target as well as the selection, so a
            // file picked here has something to show the moment it is chosen.
            onOpen={setSelected}
          />
        </div>
      </aside>

      <main className={styles.workbench__main}>
        <div className={editor.controls}>
          <button
            type="button"
            onClick={() => void run(commandIds.pickDirectory)}
          >
            Open…
          </button>
          <DirectoryField />
          <IgnoreField />
          <button type="button" onClick={() => void run(commandIds.recompute)}>
            <RefreshCw size={13} aria-hidden="true" /> Count
          </button>
        </div>

        <TabBar
          tabs={VIEWS.map((entry) => ({
            id: entry.id,
            label: entry.label,
          }))}
          active={view}
          onSelect={setView}
        />

        {/* The whole registry is iterated here and the shell knows nothing about
            how many views exist. Adding the fourth is a new file in `views/`. */}
        <ScrollArea.Root className={editor.content}>
          <ScrollArea.Viewport className={editor.content}>
            <Active generation={generation} />
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar orientation="vertical">
            <ScrollArea.Thumb />
          </ScrollArea.Scrollbar>
        </ScrollArea.Root>
      </main>

      <StatusBar
        onRecompute={() => void run(commandIds.recompute)}
        onPick={() => void run(commandIds.pickDirectory)}
      />
    </div>
  );
}

/** The directory field: local state for the keystrokes, committed on Enter.
 *
 * A controlled input bound straight to the store would rebuild every analysis on
 * each character, so the field keeps its own draft and hands it over on Enter.
 * That is the one place a component holds state, and it is transient by design.
 */
function DirectoryField() {
  const root = useClient((state) => state.root);
  const setRoot = useClient((state) => state.setRoot);

  return (
    <input
      defaultValue={root}
      key={root}
      aria-label="Directory"
      placeholder="/path/to/project"
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
      defaultValue={ignore.join(", ")}
      key={ignore.join(",")}
      aria-label="Ignore patterns"
      placeholder="ignore: target, *.snap"
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
