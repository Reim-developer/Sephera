/**
 * The view registry.
 *
 * Each analysis registers itself: an identifier, a label, a title, and the
 * component that renders it. `App.tsx` reads the registry and knows nothing about
 * how many views there are, which is what makes adding the fourth a new file
 * rather than an edit to the shell.
 *
 * This is the second idea from the IDE architecture that survives contact with a
 * client of three views: a contribution point costs one small file and saves the
 * shell from a growing switch.
 */

import type { ComponentType } from "react";
import type { ViewId } from "@/state/store";
import { DependenciesView } from "@/views/DependenciesView";
import { LocView } from "@/views/LocView";
import { SymbolsView } from "@/views/SymbolsView";

/** What a view renders with. */
export interface ViewProps {
  /** A changed value means "the answer changed". */
  generation: number;
}

/** One registered view. */
export interface ViewRegistration {
  id: ViewId;
  /** The tab's short label. */
  label: string;
  /** The heading inside the view. */
  title: string;
  component: ComponentType<ViewProps>;
}

/** Every view, in the order the tab bar shows them.
 *
 * Order is explicit rather than derived from the id, because the order is a
 * product decision -- the cheapest and most common analysis comes first -- and
 * not something an alphabetical sort should be allowed to rearrange. */
export const VIEWS: readonly ViewRegistration[] = [
  { id: "loc", label: "Lines of code", title: "Lines of code", component: LocView },
  {
    id: "symbols",
    label: "Declarations",
    title: "Declarations",
    component: SymbolsView,
  },
  {
    id: "graph",
    label: "Dependencies",
    title: "Dependencies",
    component: DependenciesView,
  },
];

/** A view by identifier. */
export function viewFor(id: ViewId): ViewRegistration | undefined {
  return VIEWS.find((view) => view.id === id);
}
