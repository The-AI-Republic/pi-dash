/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// Route module for `settings/profile/*`. The behaviour lives in `./settings`;
// this file exists only so the route has a file of its own — `mergeRoutes` in
// `app/routes/helper.ts` deduplicates route entries by `file`, so pointing two
// entries at `./settings.tsx` would silently drop one of them.
export { default } from "./settings";
