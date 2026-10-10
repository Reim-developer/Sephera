import { useEffect } from "react";
import * as ScrollArea from "@radix-ui/react-scroll-area";
import { FolderOpen, RefreshCw } from "lucide-react";

import { ActivityBar, PANELS } from "@/components/ActivityBar";
import { FilePanel } from "@/components/FilePanel";
import { FileTree } from "@/components/FileTree";
import { ProgressModal } from "@/components/ProgressModal";
import { StatusBar } from "@/components/StatusBar";
import { TabBar } from "@/components/TabBar";
import { QueryRow } from "@/views/QueryRow";
import { useGeneration } from "@/hooks/useAnalysis";
import { useActions } from "@/hooks/useActions";
import { commandIds, run } from "@/platform/commands";
import { useClient } from "@/state/store";
import { VIEWS } from "@/views/registry";
import styles from "@/styles/workbench.module.scss";
import editor from "@/styles/editor.module.scss";

/** What each analysis is doing, for the modal's label. */
const LABELS: Record<string, string> = {
  loc: "Counting lines…",
  symbols: "Counting declarations…",
  graph: "Building the dependency graph…",
};

/**
 * The workbench.
 *
 * Two columns rather than one: the project's tables on the left, the selected
 * file's detail on the right. That is the DataGrip arrangement, and it is the
 * right one for a read-only tool -- the tables answer "what is in this project"
 * and the panel answers "what is in this file", and clicking through a directory
 * answers the second without losing the answer to the first.
 *
 * The shell holds no analysis of its own. It reads the root, the view and the
 * selection, dispatches commands, and iterates the registry to find the view
 * component -- so adding the fourth analysis is a new file rather than an edit
 * here.
 */
export function App() {
  const root = useClient((state) => state.root);
  const selected = useClient((state) => state.selected);
  const setSelected = useClient((state) => state.setSelected);
  const view = useClient((state) => state.view);
  const generation = useGeneration();
  const { setView } = useActions();

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
          tabs={VIEWS.map((entry) => ({ id: entry.id, label: entry.label }))}
          active={view}
          onSelect={setView}
        />

        <div className={styles.split}>
          <section className={styles.split__grid}>
            {/* The graph's query row is the view's own, because typing a path is
                how that analysis is addressed rather than the shell's directory
                field, which applies to all three alike. */}
            {active.id === "graph" ? <QueryRow /> : null}
            <ScrollArea.Root className={styles.split__scroll}>
              <ScrollArea.Viewport className={styles.split__viewport}>
                <Active generation={generation} />
              </ScrollArea.Viewport>
              <ScrollArea.Scrollbar orientation="vertical">
                <ScrollArea.Thumb />
              </ScrollArea.Scrollbar>
            </ScrollArea.Root>
          </section>

          <aside className={styles.split__panel}>
            <FilePanel />
          </aside>
        </div>
      </main>

      <StatusBar
        onRecompute={() => void run(commandIds.recompute)}
        onPick={() => void run(commandIds.pickDirectory)}
      />

      <ProgressModal label={LABELS[view] ?? "Working…"} />
    </div>
  );
}

/** The directory field: local state for the keystrokes, committed on Enter.
 *
 * A controlled input bound straight to the store would rebuild every analysis on
 * each character, so the field keeps its own draft and hands it over on Enter.
 * That is the one place a component holds state, and it is transient by design:
 * nothing else reads it. */
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
