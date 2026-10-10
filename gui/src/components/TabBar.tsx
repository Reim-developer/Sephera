import * as Tabs from "@radix-ui/react-tabs";
import styles from "../styles/editor.module.scss";

/** A view the editor can show. */
export interface EditorTab {
  id: string;
  label: string;
}

/**
 * The editor's tab bar, on Radix's tabs primitive.
 *
 * What Radix buys: arrow-key navigation, `Home`/`End`, a correct
 * `aria-controls`/`id` pairing, and the `data-state` contract the styles use.
 * That is the whole reason to reach for a library on something this small -- a
 * hand-rolled tab bar gets the click right and the keyboard wrong.
 *
 * No close button. Every tab is a view of the same directory, so a closed tab
 * would be re-opened by the next action anyway; a control that does nothing is
 * worse than a missing one.
 */
export function TabBar({
  tabs,
  active,
  onSelect,
}: {
  tabs: readonly EditorTab[];
  active: string;
  onSelect: (id: string) => void;
}) {
  return (
    <Tabs.Root
      className={styles.tabs}
      value={active}
      onValueChange={onSelect}
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
