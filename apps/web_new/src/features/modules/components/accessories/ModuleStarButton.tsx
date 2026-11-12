// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Module favorite star (parity/specs/modules.md §4.4, MOD-016). The star
// flips at once and writes through the module favorites endpoints with
// the pinned notices; a failure rolls it back. Guests never see this
// button — the caller renders nothing for them instead of a disabled
// star — and neither do archived rows. The pressed state is the honest
// reading: set while starred, clear otherwise.

import { IconButton } from "@pidash/kit";
import type { ModuleRow } from "@pidash/api-client";
import { Star } from "lucide-react";
import * as React from "react";

import { useModuleFavorite } from "../../lib/useModuleFavorite.js";

export interface ModuleStarButtonProps {
  workspaceSlug: string;
  projectId: string;
  module: Pick<ModuleRow, "id" | "name" | "is_favorite">;
  /** Test handle the star carries. */
  testId: string;
}

export function ModuleStarButton({
  workspaceSlug,
  projectId,
  module,
  testId,
}: ModuleStarButtonProps): React.ReactElement {
  const { starred, pending, toggle } = useModuleFavorite({ workspaceSlug, projectId, module });
  return (
    <IconButton
      label={starred ? `Remove ${module.name} from favorites` : `Add ${module.name} to favorites`}
      size="small"
      aria-pressed={starred}
      disabled={pending}
      data-testid={testId}
      onClick={toggle}
    >
      <Star
        size={16}
        strokeWidth={1.5}
        aria-hidden="true"
        className={starred ? "fill-current text-(--warning)" : "text-(--text-muted)"}
      />
    </IconButton>
  );
}
