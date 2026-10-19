// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Tab body outlet (parity/specs/profile.md §5, PROF-009/010). Asks the
// registry for the active tab's page; until a slice registers one, the
// outlet shows a quiet loading state rather than a fake page.

import { Skeleton } from "@pidash/kit";
import * as React from "react";

import { useProfileSettingsPage } from "../lib/registry.js";
import type { ProfileSettingsTabId } from "../lib/tabNavigation.js";

export interface SettingsTabOutletProps {
  tab: ProfileSettingsTabId;
}

export function SettingsTabOutlet({ tab }: SettingsTabOutletProps): React.ReactElement {
  const Page = useProfileSettingsPage(tab);
  if (Page === null) {
    return (
      <div data-testid="profile-settings-tab-loading" aria-busy="true" className="flex flex-col gap-(--space-3)">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-2/3" />
      </div>
    );
  }
  return <>{Page()}</>;
}
