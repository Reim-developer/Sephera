/**
 * Layer 2: the commands.
 *
 * Every action the client can take, with an identifier, a title, and a `run`.
 * This is the one idea from the IDE architecture worth borrowing at this size: the
 * status bar, the toolbar and the keyboard shortcut all call
 * `run("sephera.recompute")`, so an action exists in exactly one place and is
 * testable without rendering a component.
 *
 * What is deliberately *not* borrowed: the command palette, the menu bar, the
 * keybinding registry. Three commands do not need three ways to invoke them.
 *
 * On the layering: a command does not import the store, and the reason is not
 * tidiness. `platform/` ranks *below* `state/`, because a command drives the store
 * and not the other way round -- an import in that direction is a cycle, and
 * `scripts/check_gui_layers.py` fails the build on it. So a command reaches the
 * store through `useActions`, the hook layer, which is the seam a test can supply.
 * The cost is that a command runs inside React, which a command in this app
 * already does: every caller is a click handler or a key listener.
 */

import { pickDirectory } from "@/services/dialog";
import { useActions } from "@/hooks/useActions";

/** The commands, by identifier. */
/** What a command can be given. */
export interface CommandContext {
  /** A directory or file, depending on the command. */
  path?: string;
}

export const commandIds = {
  recompute: "sephera.recompute",
  recomputeAll: "sephera.recomputeAll",
  pickDirectory: "sephera.pickDirectory",
  setTarget: "sephera.setTarget",
} as const;

/** What a command can be given. */
export interface CommandContext {
  /** A directory or file, depending on the command. */
  path?: string;
}

/** One command. */
interface Command {
  id: string;
  title: string;
  run: (context?: CommandContext) => void | Promise<void>;
}

/**
 * The command table.
 *
 * A plain object rather than a registry with `registerCommand`, because three
 * commands are not a contribution point. The shape is the registry's -- an id maps
 * to something with a `run` -- so adding registration later is a change to this
 * file and nothing else.
 */
export const commands: Record<string, Command> = {
  [commandIds.recompute]: {
    id: commandIds.recompute,
    title: "Recompute the analysis",
    run: () => useActions().recompute(),
  },
  [commandIds.recomputeAll]: {
    id: commandIds.recomputeAll,
    title: "Recompute every analysis",
    run: () => useActions().recomputeAll(),
  },
  [commandIds.pickDirectory]: {
    id: commandIds.pickDirectory,
    title: "Open a directory",
    run: async ({ path } = {}) => {
      // A caller that already has a path -- a test, or a deep link -- passes it
      // straight through and no dialog opens. Without one, the OS is asked.
      const chosen = path ?? (await pickDirectory());
      // A cancelled dialog resolves to `null`, and setting the root to nothing
      // would be a directory change with no directory.
      if (chosen) useActions().setRoot(chosen);
    },
  },
  [commandIds.setTarget]: {
    id: commandIds.setTarget,
    title: "Trace dependencies of a file",
    run: ({ path } = {}) => {
      if (path) useActions().setTarget(path);
    },
  },
};

/** Run a command by identifier.
 *
 * The only way a component triggers an action. A button, a status bar and a
 * keybinding all call this, so they are one code path -- which is the entire
 * point of the indirection. */
export async function run(
  id: string,
  context?: CommandContext,
): Promise<void> {
  const command = commands[id];
  if (!command) throw new Error(`no command registered as ${id}`);
  await command.run(context);
}

/** The commands, in the order a help screen would list them. */
export const commandList: readonly Command[] = [
  commands[commandIds.recompute],
  commands[commandIds.recomputeAll],
  commands[commandIds.pickDirectory],
  commands[commandIds.setTarget],
];
