// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Header creation entry on a module's detail page (parity/specs/modules.md
// §8.3, MOD-031). The page mounts it only for a standing that may add, so
// the button itself carries no permission copy: guests and a completed
// module simply never see it. It opens the shared creation dialog scoped
// to the module, the same dialog the empty card's first action opens.

import { Button } from "@pidash/kit";
import { Plus } from "lucide-react";
import * as React from "react";

import { MODULE_ADD_WORK_ITEM_LABEL } from "../lib/moduleAddWorkItem.js";

export interface ModuleAddWorkItemButtonProps {
  onOpen: () => void;
}

export function ModuleAddWorkItemButton({ onOpen }: ModuleAddWorkItemButtonProps): React.ReactElement {
  return (
    <Button variant="primary" data-testid="module-add-work-item" onClick={onOpen}>
      <Plus size={16} strokeWidth={2} aria-hidden="true" />
      {MODULE_ADD_WORK_ITEM_LABEL}
    </Button>
  );
}
