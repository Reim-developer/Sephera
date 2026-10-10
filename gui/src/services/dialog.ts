/**
 * The OS directory picker.
 *
 * A service rather than something the store does, because it is a host
 * capability: the store's job is to hold state and call services, and it does not
 * know a dialog exists. A test that needs a "user picked a directory" supplies a
 * service that resolves to a fixture path, and the store is never involved.
 */

import { open } from "@tauri-apps/plugin-dialog";

/** Ask the OS for a directory, or `null` if the user cancels. */
export async function pickDirectory(): Promise<string | null> {
  const chosen = await open({ directory: true, multiple: false });
  return typeof chosen === "string" ? chosen : null;
}
