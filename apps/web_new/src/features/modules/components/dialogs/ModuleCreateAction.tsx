// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The header's creation action (parity/specs/modules.md §2.3, MOD-003).
// The label follows the viewport, not the layout: "Add Module" where
// there is room and "Add" on small screens. The screen only mounts it for
// admins and members, so a guest never sees it. Activating it opens the
// shared create dialog through the provider's opener.

import { Button } from "@pidash/kit";
import * as React from "react";

export const MODULE_ADD_LABEL = "Add Module";
export const MODULE_ADD_LABEL_COMPACT = "Add";

export interface ModuleCreateActionProps {
  /** Small-screen viewport: the label shortens. */
  compact: boolean;
  /** Opens the create dialog. */
  onCreate: () => void;
}

export function ModuleCreateAction({ compact, onCreate }: ModuleCreateActionProps): React.ReactElement {
  return (
    <Button
      variant="primary"
      type="button"
      data-testid="modules-add"
      onClick={() => {
        onCreate();
      }}
    >
      {compact ? MODULE_ADD_LABEL_COMPACT : MODULE_ADD_LABEL}
    </Button>
  );
}
