/** A tab in the editor area.
 *
 * No close button on any of these. Every tab is a view of the same directory,
 * so a closed tab would be re-opened by the next action anyway -- a control
 * that does nothing is worse than a missing one. */
export function TabBar({
  tabs,
  active,
  onSelect,
}: {
  tabs: ReadonlyArray<{ id: string; label: string }>;
  active: string;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="editor__tabs" role="tablist">
      {tabs.map((tab) => {
        const selected = tab.id === active;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            aria-selected={selected}
            className={`editor__tab${selected ? " editor__tab--active" : ""}`}
            onClick={() => onSelect(tab.id)}
          >
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}
