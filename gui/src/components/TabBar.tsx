import * as Tabs from "@radix-ui/react-tabs";
import styles from "@/styles/editor.module.scss";

/** A tab in the editor area. */
export interface EditorTab<Id extends string = string> {
  id: Id;
  label: string;
}

/**
 * The editor's tab bar, on Radix's tabs primitive.
 *
 * What Radix buys: arrow-key navigation, `Home` and `End`, a correct
 * `aria-controls`/`id` pairing, and the `data-state` contract the styles use.
 * That is the whole reason to reach for a library on something this small -- a
 * hand-rolled tab bar gets the click right and the keyboard wrong.
 *
 * Generic over the id so the registry's `ViewId` survives the round trip. A tab
 * bar typed to `string` would accept any string and hand back `string`, which
 * throws away the one thing the registry knows.
 *
 * No close button. Every tab is a view of the same directory, so a closed tab
 * would be re-opened by the next action anyway; a control that does nothing is
 * worse than a missing one.
 */
export function TabBar<Id extends string>({
  tabs,
  active,
  onSelect,
}: {
  tabs: readonly EditorTab<Id>[];
  active: Id;
  onSelect: (id: Id) => void;
}) {
  return (
    <Tabs.Root
      className={styles.tabs}
      value={active}
      onValueChange={(value) => {
        // Radix hands back `string`, so the value is matched against the registry
        // rather than cast. A cast would let an unknown id through and render
        // nothing.
        const match = tabs.find((tab) => tab.id === value);
        if (match) onSelect(match.id);
      }}
      orientation="horizontal"
      activationMode="automatic"
    >
      <Tabs.List className={styles.tabs} aria-label="Views">
        {tabs.map((tab) => (
          <Tabs.Trigger key={tab.id} value={tab.id} className={styles.tab}>
            {tab.label}
          </Tabs.Trigger>
        ))}
      </Tabs.List>
    </Tabs.Root>
  );
}
