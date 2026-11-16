// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Exports guide registration (EXP-001, NEWFRONT-768). Importing this chunk
// puts the guide (form + history) into the workspace-settings exports
// seam; the exports section route lazy-loads it so the chunk stays out of
// the initial bundle. The exported helper lets tests re-establish the
// registration after a registry reset.
import { registerWorkspaceExportsGuide } from "../workspace-settings/index.js";

import { ExportsGuide } from "./components/ExportsGuide.js";

/** Registers the exports guide (form + history) in the settings seam. */
export function registerExportsGuide(): () => void {
  return registerWorkspaceExportsGuide(ExportsGuide);
}

registerExportsGuide();
