/**
 * The service registry.
 *
 * The application calls `services.loc`, never `locService` directly. That one
 * indirection is what makes the service layer a seam: a test replaces the whole
 * registry's contents and every caller follows, with no `vi.mock` per module and
 * no test-only branch anywhere in the application.
 *
 * It lives in `services/` rather than in `platform/` because the store imports it
 * and `platform/` sits above the store. Moving it here is what keeps the edges a
 * table with no cycle in it -- the earlier arrangement had `state/` importing
 * `platform/` while `platform/` imported `hooks/`, which imported `state/`.
 */

import {
  explorerService,
  graphService,
  locService,
  symbolsService,
} from "@/services/index";

/** Everything the client can ask the host to do. */
export interface ServiceRegistry {
  loc: typeof locService;
  symbols: typeof symbolsService;
  graph: typeof graphService;
  explorer: typeof explorerService;
}

/** The live registry. */
export const services: ServiceRegistry = {
  loc: locService,
  symbols: symbolsService,
  graph: graphService,
  explorer: explorerService,
};

/**
 * Replace the registry, for tests.
 *
 * Returns the previous contents so a caller can restore them without a second
 * fixture -- an `afterEach` that has to know what the original was is a fixture
 * that goes stale the moment a service is added.
 */
export function withServices(
  overrides: Partial<ServiceRegistry>,
): ServiceRegistry {
  const previous: ServiceRegistry = { ...services };
  Object.assign(services, overrides);
  return previous;
}
