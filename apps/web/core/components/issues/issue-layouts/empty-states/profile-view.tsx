/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useParams } from "next/navigation";
// components
import { useTranslation } from "@pi-dash/i18n";
import { EmptyStateDetailed } from "@pi-dash/propel/empty-state";

const PROFILE_EMPTY_STATE_I18N = {
  assigned: {
    title: "No work items are assigned to you",
    description: "Work items assigned to you can be tracked from here.",
  },
  created: {
    title: "No work items yet",
    description: "All work items created by you come here, track them here directly.",
  },
  subscribed: {
    title: "No work items yet",
    description: "Subscribe to work items you are interested in, track all of them here.",
  },
  activity: {
    title: "No activities yet",
    description: "Get started by creating a new work item! Add details and properties to it. Explore more in Pi Dash to see your activity.",
  },
} as const;

// TODO: If projectViewId changes, everything breaks. Figure out a better way to handle this.
export const ProfileViewEmptyState = observer(function ProfileViewEmptyState() {
  // pi dash hooks
  const { t } = useTranslation();
  // store hooks
  const { profileViewId } = useParams();

  if (!profileViewId) return null;
  const emptyState = PROFILE_EMPTY_STATE_I18N[profileViewId.toString() as keyof typeof PROFILE_EMPTY_STATE_I18N];
  if (!emptyState) return null;

  return (
    <EmptyStateDetailed
      assetKey="work-item"
      title={t(emptyState.title)}
      description={t(emptyState.description)}
    />
  );
});
