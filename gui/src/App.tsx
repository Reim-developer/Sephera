import { useEffect } from "react";
import * as ScrollArea from "@radix-ui/react-scroll-area";

import { EmptyState } from "@/components/EmptyState";
import { FilePanel } from "@/components/FilePanel";
import { FileTree } from "@/components/FileTree";
import { FileViewer } from "@/components/FileViewer";
import { ProgressModal } from "@/components/ProgressModal";
import { SettingsPanel } from "@/components/SettingsPanel";
import { TopNav } from "@/components/TopNav";
import { QueryRow } from "@/views/QueryRow";
import { useGeneration } from "@/hooks/useAnalysis";
import { commandIds, run } from "@/platform/commands";
import { useClient } from "@/state/store";
import { VIEWS } from "@/views/registry";
import styles from "@/styles/workbench.module.scss";

/** What each analysis is doing, for the modal's label. */
const LABELS: Record<string, string> = {
  loc: "Counting lines…",
  symbols: "Counting declarations…",
  graph: "Building the dependency graph…",
};

/**
 * The workbench.
 *
 * A navigation bar across the top, a side panel beside it, and one content
 * column -- the DataGrip arrangement, and the right one for a read-only tool:
 * the tables answer "what is in this project", the panel on the right answers
 * "what is in this file", and clicking through a directory answers the second
 * without losing the answer to the first.
 *
 * The navigation is a row of the shell, not part of the content area. It used to
 * be in the content area, which meant opening a file replaced it with the file's
 * text and left no way to change what was being counted without closing the file
 * first. Two rows and they stay: the folder and patterns on one, the analysis on
 * the other.
 *
 * The shell holds no analysis of its own. It reads the root, the view, the
 * selection and the panel, dispatches commands, and iterates the registry to
 * find the view component -- so adding the fourth analysis is a new file rather
 * than an edit here.
 */
export function App() {
  const root = useClient((state) => state.root);
  const selected = useClient((state) => state.selected);
  const setSelected = useClient((state) => state.setSelected);
  const view = useClient((state) => state.view);
  const panel = useClient((state) => state.panel);
  const generation = useGeneration();

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
      <TopNav />

      <div className={styles.workbench__body}>
        {root && panel === "explorer" ? (
          <aside className={styles.sidebar}>
            <h2 className={styles.sidebar__title}>Explorer</h2>
            <div className={styles.sidebar__body}>
              <FileTree
                root={root}
                selected={selected}
                onOpen={setSelected}
              />
            </div>
          </aside>
        ) : null}

        {root && panel === "settings" ? (
          <aside className={styles.sidebar}>
            <SettingsPanel />
          </aside>
        ) : null}

        <main className={styles.workbench__main}>
          {root ? (
            <div className={styles.split}>
              <section className={styles.split__grid}>
                {/* The graph's query row is the view's own, because typing a path
                    is how that analysis is addressed rather than the shell's
                    directory field, which applies to all three alike. */}
                {view === "graph" && !selected ? <QueryRow /> : null}
                <ScrollArea.Root className={styles.split__scroll}>
                  <ScrollArea.Viewport className={styles.split__viewport}>
                    {selected ? (
                      <FileViewer />
                    ) : (
                      <Active generation={generation} />
                    )}
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
          ) : (
            <EmptyState />
          )}
        </main>
      </div>

      <ProgressModal label={LABELS[view] ?? "Working…"} />
    </div>
  );
}
