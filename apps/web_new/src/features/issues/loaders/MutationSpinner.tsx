// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Floating mutation spinner (parity/specs/issues.md §1, ISS-005). A small
// square docked top-right holding a status spinner while a filter or
// display refetch is in flight. The layout root mounts it only while the
// mutation loader is raised.

import { Spinner } from "@pidash/kit";
import * as React from "react";

export function MutationSpinner(): React.ReactElement {
  return (
    <div
      data-testid="issues-mutation-spinner"
      className="fixed top-(--space-4) right-(--space-4) z-50 flex size-9 items-center justify-center rounded-(--radius-control) border border-(--border) bg-(--bg)"
    >
      <Spinner size="small" label="Updating issues" />
    </div>
  );
}
