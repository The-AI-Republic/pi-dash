// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-home tour seam (AUTH-042). The registration the shell mounts
// on the home route: it gates the product-tour overlay on the profile's
// tour flag (loaded profile, flag still false — once per user) and wires
// every completion or dismissal to the flag write. The projects area
// passes its create-project dialog opener through; this slice owns the
// overlay plus the wiring, not home UI. Written from parity/specs/auth.md.

import * as React from "react";

import { useMe, useProfile } from "../../../core/session/index.js";
import { useCompleteTour } from "../api.js";
import { shouldShowProductTour } from "../tour.js";
import { ProductTour } from "./ProductTour.js";

export interface WorkspaceHomeTourProps {
  /**
   * Opens the create-project dialog from the tour's last stop. Wired by
   * the projects area through the shell; until then the last stop only
   * completes the tour.
   */
  onCreateProject?: (() => void) | undefined;
}

export function WorkspaceHomeTour({ onCreateProject }: WorkspaceHomeTourProps): React.ReactElement | null {
  const me = useMe();
  const profile = useProfile();
  const complete = useCompleteTour();

  if (!shouldShowProductTour(profile.data)) return null;
  return (
    <ProductTour
      firstName={me.data?.first_name ?? ""}
      onComplete={() => complete.mutate()}
      {...(onCreateProject === undefined ? {} : { onCreateProject })}
    />
  );
}
