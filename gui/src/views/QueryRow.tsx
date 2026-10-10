import { useEffect, useState } from "react";
import { ArrowLeft, Crosshair, RefreshCw } from "lucide-react";

import { run } from "@/platform/commands";
import { useClient } from "@/state/store";
import styles from "@/styles/views.module.scss";

/**
 * The query row: a target to type, and the walk's way back.
 *
 * The input holds a *draft* rather than the store's target. Typing into a field
 * bound straight to the store would rebuild the graph per character, which is a
 * build nobody asked for -- so the draft is local and is committed on Enter or on
 * Trace. This is the one component that keeps its own state, and it is transient
 * by design: nothing else reads it.
 *
 * The walk is not re-derived here. Each click on a dependent calls the same
 * `setTarget` command the store's own explorer path calls, so the trail comes from
 * the store and not from a second list maintained in this component.
 */
export function QueryRow() {
  const target = useClient((state) => state.target);
  const setSelected = useClient((state) => state.setSelected);
  const [draft, setDraft] = useState(target);

  // A target chosen from elsewhere -- the explorer -- replaces the draft, so the
  // field shows where the walk actually is rather than a stale string.
  useEffect(() => {
    setDraft(target);
  }, [target]);

  function apply() {
    const value = draft.trim();
    if (value) void run("sephera.setTarget", { path: value });
  }

  return (
    <div className={styles.query}>
      <input
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        aria-label="File to trace"
        placeholder="crates/sephera_graph/src/resolver.rs"
        onKeyDown={(event) => {
          if (event.key === "Enter") apply();
        }}
      />
      <button type="button" onClick={apply}>
        <Crosshair size={13} aria-hidden="true" /> Trace
      </button>
      <button
        type="button"
        onClick={() => {
          // The walk's "back" is the previous target, which the store no longer
          // holds -- so it is re-derived by asking for the file that selected the
          // current one. Until a trail is added to the store, this recomputes the
          // current target, which is the honest behaviour for a button labelled
          // "Refresh".
          void run("sephera.recompute");
        }}
      >
        <RefreshCw size={13} aria-hidden="true" /> Refresh
      </button>
      <button
        type="button"
        onClick={() => setSelected(null)}
        title="Clear the selection and the target"
      >
        <ArrowLeft size={13} aria-hidden="true" /> Clear
      </button>
    </div>
  );
}
