// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Local-chat capability predicate (NEWPIDASH-300). Kept in its own module
// because every consumer is lazily loaded (the desktop assistant routes,
// the assistant layout and the runners chat): defining it in ./index.ts
// would pin it into the shared chunk the web initial bundle (size gate)
// statically imports.

import { platform } from "./index.js";

/** True when this build can serve chat from the machine itself. */
export function hasLocalChat(): boolean {
  return platform.localChat !== undefined;
}
