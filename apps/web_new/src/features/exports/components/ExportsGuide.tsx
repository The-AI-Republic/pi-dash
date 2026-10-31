// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Exports guide (parity/specs/exports.md §1, EXP-001). The settings frame
// (NEWFRONT-548) owns the heading, explainer and gate; this is the guide
// the exports area registers into its seam: the request form above the
// previous-exports history. Mounted through the seam, never by a route.
import * as React from "react";

import type { WorkspaceExportsGuideProps } from "../../workspace-settings/index.js";
import { ExportsForm } from "./ExportsForm.js";
import { ExportsHistory } from "./ExportsHistory.js";

/** The exports settings body: form on top, previous exports below. */
export function ExportsGuide({ workspaceSlug }: WorkspaceExportsGuideProps): React.ReactElement {
  return (
    <>
      <ExportsForm workspaceSlug={workspaceSlug} />
      <ExportsHistory workspaceSlug={workspaceSlug} />
    </>
  );
}
