/**
 * Layer 1: reading a file in pieces, for the viewer.
 *
 * A separate service rather than a call from the component, because a component
 * that reaches the host has no seam a test can substitute -- and the viewer is
 * the one place where the interesting behaviour is at the boundary between chunk
 * and chunk, which is exactly what a test needs to drive.
 *
 * The first version of the viewer called `invoke` directly from the component.
 * It worked and it crossed the layering: `components/` may import `lib/`, not
 * the host. `scripts/check_gui_layers.py` passed on it, because the rule allows
 * it -- the rule is right and the design was wrong. So the read moved into this
 * service and the component now goes through `hooks/`, the same route every other
 * component takes.
 */

import { COMMANDS, invoke } from "@/lib/ipc";
import type { FileChunk } from "@/lib/ipc";

/** Reading a file a window at a time. */
export const viewerService = {
  async readChunk(
    root: string,
    path: string,
    offset: number,
    length: number,
  ): Promise<FileChunk> {
    return invoke<FileChunk>(COMMANDS.readFileChunk, {
      root,
      path,
      offset,
      length,
    });
  },
};
