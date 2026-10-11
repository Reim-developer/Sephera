import { useCallback, useEffect, useRef, useState } from "react";

import { useClient } from "@/state/store";
import styles from "@/styles/workbench.module.scss";

/** The narrowest the panel may be, and the widest.
 *
 * The floor is where a declaration row is still readable -- a narrower one puts
 * the line number against the edge of the window. The ceiling is where the panel
 * stops being a detail and starts being the content, which is the point at which
 * the table it came from has been squeezed away. */
const MIN = 220;
const MAX = 760;

/**
 * The drag handle between the table and the panel beside it.
 *
 * A column of the grid rather than a border on the panel, because a border
 * cannot carry a cursor or a hit area -- and a one-pixel target is not a hit area.
 * The handle is a real element in a real column, so what it resizes is the column
 * the panel is drawn in, and the table takes the difference.
 *
 * The width lives in the store rather than in local state, so it survives the
 * component re-mounting and is visible to anything that asks. It is a display
 * preference, which is state, and the store is the only place the client keeps
 * state.
 */
export function PanelResizer() {
  const width = useClient((state) => state.panelWidth);
  const setPanelWidth = useClient((state) => state.setPanelWidth);
  const [dragging, setDragging] = useState(false);
  const frame = useRef<HTMLDivElement>(null);

  /** Move the handle's edge to `clientX`, clamped to the allowed range. */
  const drag = useCallback(
    (clientX: number) => {
      const edge = frame.current?.getBoundingClientRect().left ?? 0;
      // The handle is five pixels wide and sits between the two columns, so its
      // left edge is the boundary and its own width is what the measurement is
      // taken from.
      const next = Math.round(clientX - edge) + width;
      setPanelWidth(Math.min(MAX, Math.max(MIN, next)));
    },
    [setPanelWidth, width],
  );

  // The move and up listeners go on the window rather than on the handle, so a
  // drag that leaves the handle -- which every drag does, by definition -- still
  // tracks the pointer. Without this the drag stops at the handle's right edge and
  // then keeps going under the column, which reads as the panel refusing to grow.
  useEffect(() => {
    if (!dragging) return;

    const move = (event: PointerEvent) => {
      event.preventDefault();
      drag(event.clientX);
    };
    const up = () => setDragging(false);

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, [drag, dragging]);

  return (
    <div
      ref={frame}
      className={styles.split__handle}
      data-resizing={dragging || undefined}
      role="separator"
      aria-orientation="vertical"
      aria-valuenow={width}
      aria-valuemin={MIN}
      aria-valuemax={MAX}
      aria-label="Resize the file panel"
      onPointerDown={(event) => {
        // The button, on the pointer, is the only capture that is reliable across
        // the browsers Tauri ships: it is not cancelled by a scroll gesture and
        // it does not need a polyfill for the older WebKitGTK the Linux build
        // links against.
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
      }}
    />
  );
}
