/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// Empty in OSS. Cloud (or other distributions) replace this file via
// build-time overlay to call `registerAxiosSetup(...)` from
// `../_axios-setup` and install per-instance interceptors (eg. a 401 →
// refresh-token retry handler). Keep this file side-effect-only.
//
// Important: import `registerAxiosSetup` from `../_axios-setup`, NOT
// from `../api.service`. Importing from `api.service` re-enters its
// evaluation while it is awaiting this very side-effect import, and
// the registry `const` would be in the TDZ.

// Type-only export so this file is a valid ES module (and `import
// "./ee/init"` from `api.service` resolves). Erased at compile time;
// nothing ships in the OSS bundle.
export type _Stub = never;
