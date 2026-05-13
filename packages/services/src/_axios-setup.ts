/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// Lives in its own module — not on `api.service` — so the side-effect
// seam at `api.service` → `./ee/init` doesn't cycle back through
// `api.service` to look up the registry. A `const` registry on
// `api.service` would be in the TDZ at the moment `ee/init` evaluates,
// throwing `ReferenceError: Cannot access 'axiosSetups' before
// initialization` at module load. Putting the registry here breaks the
// cycle: `ee/init` imports from this file directly, which finishes
// initializing before `ee/init`'s body runs.

import type { AxiosInstance } from "axios";

export type AxiosInstanceSetup = (instance: AxiosInstance) => void;

const axiosSetups: AxiosInstanceSetup[] = [];

/**
 * Register a function that runs against every new axios instance created
 * by an `APIService` subclass. Must be called before the first service
 * is constructed — the intended path is the `./ee/init` side-effect
 * seam that `api.service` imports at the top of its module.
 */
export function registerAxiosSetup(setup: AxiosInstanceSetup): void {
  axiosSetups.push(setup);
}

/** Apply every registered setup to a freshly-created axios instance. */
export function applyAxiosSetups(instance: AxiosInstance): void {
  for (const setup of axiosSetups) setup(instance);
}
