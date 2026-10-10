import type { ReactNode } from "react";

/** One icon in the activity bar. */
export interface ActivityItem {
  id: string;
  label: string;
  /** A short glyph, drawn in the client rather than imported as an asset. */
  glyph: string;
}

/**
 * The left rail that chooses which sidebar panel is open.
 *
 * One selected item at a time, and clicking the selected one closes the panel --
 * the behaviour Visual Studio Code has, and the reason a second click is not a
 * no-op: with a sidebar this narrow, hiding it is how a reclaims the width.
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
    <nav className="activity-bar" aria-label="Views">
      {items.map((item) => {
        const selected = active === item.id;
        return (
          <button
            key={item.id}
            type="button"
            className={`activity-bar__item${selected ? " activity-bar__item--active" : ""}`}
            aria-pressed={selected}
            aria-label={item.label}
            title={item.label}
            onClick={() => onSelect(selected ? null : item.id)}
          >
            <span aria-hidden="true">{item.glyph}</span>
          </button>
        );
      })}
    </nav>
  );
}

/** A section inside a panel: a collapsible group with a titled body. */
export function PanelGroup({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="sidebar__group">
      <div className="sidebar__group-header">
        <span aria-hidden="true">▾</span>
        {title}
      </div>
      <div className="sidebar__group-body">{children}</div>
    </section>
  );
}
