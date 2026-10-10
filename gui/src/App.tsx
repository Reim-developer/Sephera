import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as ScrollArea from "@radix-ui/react-scroll-area";
import * as Separator from "@radix-ui/react-separator";
import { FolderOpen, PanelRightClose, RefreshCw } from "lucide-react";

import { ActivityBar, PANELS } from "./components/ActivityBar";
import { FileTree } from "./components/FileTree";
import { StatusBar } from "./components/StatusBar";
import { TabBar } from "./components/TabBar";
import { LocView } from "./views/LocView";
import { SymbolsView } from "./views/SymbolsView";
import { countLines } from "./lib/ipc";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import styles from "./styles/workbench.module.scss";
import editor from "./styles/editor.module.scss";
import views from "./styles/views.module.scss";

/** The views the editor can show. */
const TABS = [
  { id: "loc", label: "Lines of code" },
  { id: "symbols", label: "Declarations" },
  { id: "graph", label: "Dependencies" },
] as const;

export function App() {
  const [root, setRoot] = useState<string>(
    () => new URLSearchParams(location.search).get("path") ?? ".",
  );
  const [ignore, setIgnore] = useState("");
  const [panel, setPanel] = useState<string | null>("explorer");
  const [tab, setTab] = useState<string>("loc");
  const [selected, setSelected] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [summary, setSummary] = useState<string | null>(null);
  const [config, setConfig] = useState<string | null>(null);
  const [sidebarWidth, setSidebarWidth] = useState(260);
  const dragging = useRef(false);

  const patterns = useMemo(
    () =>
      ignore
        .split(",")
        .map((entry) => entry.trim())
        .filter((entry) => entry.length > 0),
    [ignore],
  );

  // The status line is the count's own summary, refreshed on the same token the
  // views use. Reading it from here rather than passing a callback into a view
  // keeps the view free of anything that is not rendering.
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const view = await countLines(root, patterns);
        if (cancelled) return;
        setSummary(
          `${view.files_scanned.toLocaleString()} files, ${view.rows.length} languages, ${view.elapsed_ms} ms`,
        );
        setConfig(view.config_source);
      } catch {
        if (!cancelled) setSummary(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [root, patterns, reloadToken]);

  const reopen = useCallback(
    () => setReloadToken((token) => token + 1),
    [],
  );

  // Drag to resize the sidebar, with the handle hidden until a drag starts. The
  // width is applied to the sidebar directly rather than through React state on
  // every mousemove, so a drag does not re-render the tree at sixty frames a
  // second.
  const onHandleDown = useCallback((event: React.PointerEvent) => {
    event.preventDefault();
    dragging.current = true;
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  }, []);

  const onHandleMove = useCallback((event: React.PointerEvent) => {
    if (!dragging.current) return;
    setSidebarWidth(
      Math.min(520, Math.max(180, event.clientX - 48)),
    );
  }, []);

  const onHandleUp = useCallback(() => {
    dragging.current = false;
  }, []);

  async function pickDirectory() {
    const chosen = await openDialog({ directory: true, multiple: false });
    if (typeof chosen === "string") setRoot(chosen);
  }

  // Ctrl+R recomputes. The default browser binding is suppressed at the window
  // level, because in a Tauri app there is no page to reload and a Ctrl+R that
  // does nothing is worse than one that recomputes.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "F5" || (event.ctrlKey && event.key === "r")) {
        event.preventDefault();
        reopen();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [reopen]);

  // Ctrl+Shift+O opens the directory picker: the one action worth a shortcut,
  // because it is what a user does first and most often.
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.ctrlKey && event.shiftKey && event.key === "O") {
        event.preventDefault();
        void pickDirectory();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className={styles.workbench}>
      <ActivityBar items={PANELS} active={panel} onSelect={setPanel} />

      {panel ? (
        <aside className={styles.sidebar} style={{ width: sidebarWidth }}>
          <h2 className={styles.sidebar__title}>
            <FolderOpen size={12} aria-hidden="true" /> {panel}
          </h2>
          <div className={styles.sidebar__body}>
            {panel === "explorer" ? (
              <FileTree
                root={root}
                onOpen={setSelected}
                selected={selected}
              />
            ) : (
              <ConfigPanel root={root} patterns={patterns} />
            )}
          </div>
          <div
            className={styles.sidebar__handle}
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize sidebar"
            onPointerDown={onHandleDown}
            onPointerMove={onHandleMove}
            onPointerUp={onHandleUp}
          />
        </aside>
      ) : null}

      <main className={styles.workbench__main}>
        <div className={editor.controls}>
          <button type="button" onClick={() => void pickDirectory()}>
            <PanelRightClose size={13} aria-hidden="true" /> Open…
          </button>
          <input
            value={root}
            onChange={(event) => setRoot(event.target.value)}
            aria-label="Directory"
            placeholder="/path/to/project"
            onKeyDown={(event) => {
              if (event.key === "Enter") reopen();
            }}
          />
          <input
            value={ignore}
            onChange={(event) => setIgnore(event.target.value)}
            aria-label="Ignore patterns"
            placeholder="ignore: target, *.snap"
            onKeyDown={(event) => {
              if (event.key === "Enter") reopen();
            }}
          />
          <button type="button" onClick={reopen}>
            <RefreshCw size={13} aria-hidden="true" /> Count
          </button>
        </div>

        <TabBar tabs={TABS} active={tab} onSelect={setTab} />

        <ScrollArea.Root className={editor.content}>
          <ScrollArea.Viewport className={editor.content}>
            {tab === "loc" ? (
              <LocView
                path={root}
                ignore={patterns}
                reloadToken={reloadToken}
              />
            ) : tab === "symbols" ? (
              <SymbolsView
                path={root}
                ignore={patterns}
                reloadToken={reloadToken}
              />
            ) : (
              <NotBuilt label={labelFor(tab)} />
            )}
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar orientation="vertical">
            <ScrollArea.Thumb />
          </ScrollArea.Scrollbar>
        </ScrollArea.Root>
      </main>

      <StatusBar
        summary={summary}
        config={config}
        busy={false}
        onRecompute={reopen}
      />

      {/* Radix's separator, used where the shell needs a rule rather than a
          border: it is one element with a role, and `decorative` where the rule
          is visual only. */}
      <Separator.Root decorative style={{ display: "none" }} />
    </div>
  );
}

/** The label for a tab id, without the caller having to know the table. */
function labelFor(id: string): string {
  return TABS.find((tab) => tab.id === id)?.label ?? id;
}

/** The configuration panel: which file was read, and what it contributed. */
function ConfigPanel({
  root,
  patterns,
}: {
  root: string;
  patterns: string[];
}) {
  return (
    <div className={styles.tree}>
      <div className={styles.sidebar__groupHeader}>Configuration</div>
      <p className={styles.sidebar__empty}>
        Reads <code>.sephera.toml</code> from {root}. Patterns from the file are
        applied first, then the {patterns.length} typed here.
      </p>
    </div>
  );
}

/** A view that is not built yet, in the shape the real ones use. */
function NotBuilt({ label }: { label: string }) {
  return (
    <div className={views.placeholder}>
      <span className={views.placeholder__title}>{label}</span>
      <span className={views.placeholder__hint}>
        The command and its types already exist in{" "}
        <code>gui/src-tauri/src/commands/graph.rs</code> and{" "}
        <code>gui/src/lib/ipc.ts</code>. Only the React is missing, and building
        it is the next round.
      </span>
    </div>
  );
}
