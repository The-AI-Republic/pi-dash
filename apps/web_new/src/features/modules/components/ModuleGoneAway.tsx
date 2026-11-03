// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Gone-away view (parity/specs/modules.md §8.1, MOD-029): a missing,
// deleted, or archived-via-live-address module renders this dedicated
// view — never a generic 404, never the layout chrome, never an
// auto-redirect — with a working route back to the modules list.

import { EmptyState } from "@pidash/kit";
import * as React from "react";

export interface ModuleGoneAwayProps {
  onBack: () => void;
}

export function ModuleGoneAway({ onBack }: ModuleGoneAwayProps): React.ReactElement {
  return (
    <div data-testid="modules-gone-away">
      <EmptyState
        variant="detailed"
        art={{ category: "general", type: "empty" }}
        title="Module does not exist"
        description="This module was deleted, or the address points at another project. Go back to the project's modules list."
        primaryAction={{ label: "Back to modules", onSelect: () => onBack() }}
      />
    </div>
  );
}
