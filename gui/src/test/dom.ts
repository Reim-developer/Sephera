/**
 * Vitest's DOM setup.
 *
 * Three things, each of which a test otherwise hits on its first render rather
 * than on its first failure:
 *
 * - `cleanup` between tests. `globals` is `false`, so `@testing-library/react`
 *   cannot register its own `afterEach` hook, and without this every render stays
 *   in the document -- which surfaces as "found multiple elements" on the second
 *   test rather than as a leak anyone would diagnose.
 * - `window.matchMedia`, read by Radix and by nothing in this client's own code,
 *   so a missing stub fails with a message that names Radix.
 * - `IntersectionObserver`, a `ScrollArea` dependency, absent from jsdom.
 *
 * Stubbed here rather than in a test, so the tests stay about the boundary they
 * exist to check.
 */

import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";

afterEach(() => {
  cleanup();
});

beforeEach(() => {
  if (!window.matchMedia) {
    vi.stubGlobal(
      "matchMedia",
      vi.fn().mockReturnValue({
        matches: false,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    );
  }

  if (!("IntersectionObserver" in window)) {
    vi.stubGlobal(
      "IntersectionObserver",
      vi.fn().mockReturnValue({
        observe: vi.fn(),
        unobserve: vi.fn(),
        disconnect: vi.fn(),
      }),
    );
  }
});
