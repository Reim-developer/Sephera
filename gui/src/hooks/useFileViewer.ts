/**
 * Layer 4: the viewer's data, as a hook.
 *
 * `components/FileViewer.tsx` renders; this reads. The same split as
 * `useFileTree.ts`, and for the same reason: a component that calls the host has
 * no seam a test can substitute. `services/viewer.ts` is reachable from a *hook*
 * and not from a component, and `scripts/check_gui_layers.py` enforces that
 * edge -- the viewer crossed it in its first version by calling `invoke` from the
 * component, and the check let it through because `lib/` is an allowed import.
 * The rule was right and the design was wrong.
 *
 * So the windowing state lives here: how far the file has been read, how big it
 * is, and the read that failed. What the component gets is text and a "load
 * more" it can call.
 */

import { useCallback, useEffect, useState } from "react";

import type { FileChunk } from "@/lib/ipc";
import { services } from "@/services/registry";
import { useClient } from "@/state/store";

/** What the viewer needs before it can render. */
export interface FileViewerData {
  /** The text read so far, in source order. */
  text: string;
  /** How many bytes that is, which is also the next window's offset. */
  loaded: number;
  /** The file's size in bytes. */
  total: number;
  /** A read that failed, or `null`. */
  error: string | null;
  /** Read one more window. */
  loadMore: () => void;
}

/** How many bytes one read takes.
 *
 * The same number for the first read and every later one, because they are the
 * same operation. A source file is a few kilobytes and a log can be a gigabyte,
 * so the window is small enough that opening either is quick, and "load more" is
 * a button rather than an infinite scroll: reading the rest of a 200 MB file
 * should be a decision, not something that happens because someone flicked a
 * wheel. */
const CHUNK_BYTES = 64 * 1024;

/**
 * Read the selected file, a window at a time.
 *
 * The selection is the dependency, so changing files resets the read -- and a
 * stale window from the previous file would otherwise sit above the new one's
 * opening lines. Cancelled by a flag rather than by ignoring the reply, because
 * React's StrictMode double-invokes effects in development and a half-written
 * file is worse than a late one.
 */
export function useFileViewer(): FileViewerData {
  const root = useClient((state) => state.root);
  const selected = useClient((state) => state.selected);
  const [text, setText] = useState("");
  const [loaded, setLoaded] = useState(0);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!selected) {
      setText("");
      setLoaded(0);
      return;
    }
    let cancelled = false;
    setText("");
    setLoaded(0);
    setError(null);

    void (async () => {
      try {
        const first = await services.viewer.readChunk(
          root,
          selected,
          0,
          CHUNK_BYTES,
        );
        if (cancelled) return;
        setText(first.text);
        setLoaded(first.bytes);
        setTotal(first.total);
      } catch (cause) {
        if (!cancelled) setError(String(cause));
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [root, selected]);

  const loadMore = useCallback(() => {
    if (!selected) return;
    void (async () => {
      try {
        const step = Math.min(total - loaded, CHUNK_BYTES);
        if (step <= 0) return;
        const next = await services.viewer.readChunk(
          root,
          selected,
          loaded,
          step,
        );
        setText((previous) => previous + next.text);
        setLoaded(next.bytes);
      } catch (cause) {
        setError(String(cause));
      }
    })();
  }, [loaded, root, selected, total]);

  return { text, loaded, total, error, loadMore };
}

/** One window, as the host sends it. */
export type { FileChunk };
