import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  open as openDialog,
} from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { ActivityBar, type ActivityItem } from "./components/ActivityBar";
import { FileTree } from "./components/FileTree";
import { StatusBar } from "./components/StatusBar";
import { TabBar } from "./components/TabBar";
import { LocView } from "./views/LocView";
import { countLines } from "./lib/ipc";

/** The views the editor can show. */
const TABS = [
  { id: "loc", label: "Lines of code" },
  { id: "symbols", label: "Declarations" },
  { id: "graph", label: "Dependencies" },
] as const;

/** The activity bar's panels. `explorer` is the only one with content today. */
const PANELS: readonly ActivityItem[] = [
  { id: "explorer", label: "Explorer", glyph: "📄" },
  { id: "search", label: "Search", glyph: "🔍" },
  { id: "config", label: "Configuration", glyph: "⚙" },
];

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
  // views use. Reading it from here rather than passing a callback into the view
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

  const reopen = useCallback(() => setReloadToken((token) => token + 1), []);

  // Drag to resize the sidebar, with the handle hidden until a drag starts. The
  // width is applied to the sidebar directly rather than through React state on
  // each mousemove, so a drag does not re-render the tree at sixty frames a
  // second.
  const onHandleDown = useCallback((event: React.PointerEvent) => {
    event.preventDefault();
    dragging.current = true;
    (event.target as HTMLElement).setPointerCapture(event.pointerId);
  }, []);

  const onHandleMove = useCallback((event: React.PointerEvent) => {
    if (!dragging.current) return;
    const width = event.clientX - 48;
    setSidebarWidth(Math.min(520, Math.max(180, width)));
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

  // Ctrl+Shift+O opens the directory picker, which is the one action worth a
  // shortcut: it is the thing a user does first and most often.
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

  // Dragging the window by the empty area of the editor, because a Tauri window
  // with `decorations: true` already has a title bar and this client's own
  // regions should not fight it. Left out deliberately.
  void getCurrentWindow;

  return (
    <div className="workbench">
      <ActivityBar items={PANELS} active={panel} onSelect={setPanel} />

      {panel ? (
        <aside className="sidebar" style={{ width: sidebarWidth }}>
          <h2 className="sidebar__title">{panel}</h2>
          <div className="sidebar__body">
            {panel === "explorer" ? (
              <FileTree root={root} onOpen={setSelected} selected={selected} />
            ) : panel === "config" ? (
              <ConfigPanel root={root} patterns={patterns} />
            ) : (
              <Placeholder title="Search" hint="Not implemented yet." />
            )}
          </div>
          <div
            className="sidebar__handle"
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize sidebar"
            onPointerDown={onHandleDown}
            onPointerMove={onHandleMove}
            onPointerUp={onHandleUp}
          />
        </aside>
      ) : null}

      <main className="workbench__main">
        <header className="editor__controls">
          <button type="button" onClick={() => void pickDirectory()}>
            Open…
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
            Count
          </button>
        </header>

        <TabBar tabs={TABS} active={tab} onSelect={setTab} />

        <div className="editor__content">
          {tab === "loc" ? (
            <LocView path={root} ignore={patterns} reloadToken={reloadToken} />
          ) : (
            <Placeholder
              title={TABS.find((entry) => entry.id === tab)?.label ?? ""}
              hint="This view is not implemented yet."
            />
          )}
        </div>
      </main>

      <StatusBar
        summary={summary}
        config={config}
        busy={false}
      />
    </div>
  );
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
    <div className="sidebar__group">
      <div className="sidebar__group-header">Configuration</div>
      <p className="sidebar__empty">
        Reads <code>.sephera.toml</code> from {root}. Patterns from the file are
        applied first, then the {patterns.length} typed here.
      </p>
    </div>
  );
}

function Placeholder({ title, hint }: { title: string; hint: string }) {
  return (
    <div className="placeholder">
      <span className="placeholder__title">{title}</span>
      <span className="placeholder__hint">{hint}</span>
    </div>
  );
}
