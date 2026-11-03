// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Archives tab-content slot (parity/specs/archives.md §1.5). The shell
// owns this region on every archives screen; each tab slice mounts its
// list, skeleton, empty and no-match states into it. The driver scopes
// its skeleton and content probes to this region.

import * as React from "react";

export interface ArchivesTabSlotProps {
  children?: React.ReactNode | undefined;
}

export function ArchivesTabSlot({ children }: ArchivesTabSlotProps): React.ReactElement {
  return <div data-testid="archives-content">{children}</div>;
}
