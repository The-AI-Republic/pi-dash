// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Profile tab body (parity/specs/profile.md §1–§3, PROF-002/006/007). One
// route serves the route segment: the three issue tabs render the
// ownership list (guests meet the inline denial), the activity tab renders
// the paged feed with the download control reserved for admins and
// members, and any other segment renders nothing while the frame stays.

import { useQuery } from "@tanstack/react-query";
import { createFileRoute } from "@tanstack/react-router";
import * as React from "react";

import { getAppClient } from "../../../../../core/api/client.js";
import { useMe, usePermissions } from "../../../../../core/session/index.js";
import {
  PROFILE_ACTIVITY_PAGE_SIZE,
  profileActivityQueryOptions,
} from "../../../../../features/profile/api/queries.js";
import { ProfileAccessDenied } from "../../../../../features/profile/components/ProfileAccessDenied.js";
import { ProfileActivityDownload } from "../../../../../features/profile/components/ProfileActivityDownload.js";
import { ProfileActivityTab } from "../../../../../features/profile/components/ProfileActivityList.js";
import { ProfileIssuesTab } from "../../../../../features/profile/components/ProfileIssuesTab.js";
import {
  PROFILE_DENIED_TABS,
  profileTabBySegment,
  type ProfileTabKey,
} from "../../../../../features/profile/lib/tabs.js";

import type { UserActivityPage } from "@pidash/api-client";

export const Route = createFileRoute("/$ws/_gated/profile/$userId/$tabId")({
  loader: async ({ context, params }) => {
    if (params.tabId === "activity") {
      await context.queryClient
        .ensureQueryData(
          profileActivityQueryOptions(context.apiClient, params.ws, params.userId, PROFILE_ACTIVITY_PAGE_SIZE)
        )
        .catch(() => undefined);
    }
    return {};
  },
  component: ProfileTabComponent,
});

const ISSUE_TITLES: Record<"assigned" | "created" | "subscribed", string> = {
  assigned: "Assigned",
  created: "Created",
  subscribed: "Subscribed",
};

function isDeniedTab(tab: ProfileTabKey | null): boolean {
  return tab !== null && PROFILE_DENIED_TABS.includes(tab);
}

function ProfileTabComponent(): React.ReactElement | null {
  const { ws, userId, tabId } = Route.useParams();
  const permissions = usePermissions(ws);
  const me = useMe();
  const tab = profileTabBySegment(tabId);
  if (tab === null) return null;
  if (isDeniedTab(tab.key) && !permissions.canEdit) return <ProfileAccessDenied />;
  if (tab.key === "assigned" || tab.key === "created" || tab.key === "subscribed") {
    return <ProfileIssuesTab workspaceSlug={ws} userId={userId} ownership={tab.key} title={ISSUE_TITLES[tab.key]} />;
  }
  if (tab.key === "activity") {
    return (
      <ActivityScreen
        workspaceSlug={ws}
        userId={userId}
        currentUserId={me.data?.id ?? null}
        canDownload={permissions.canEdit}
      />
    );
  }
  return null;
}

function ActivityScreen({
  workspaceSlug,
  userId,
  currentUserId,
  canDownload,
}: {
  workspaceSlug: string;
  userId: string;
  currentUserId: string | null;
  canDownload: boolean;
}): React.ReactElement {
  const client = getAppClient();
  const page = useQuery({
    ...profileActivityQueryOptions(client, workspaceSlug, userId, PROFILE_ACTIVITY_PAGE_SIZE),
    throwOnError: false,
  });
  const initial: UserActivityPage | undefined = page.data;
  return (
    <ProfileActivityTab
      workspaceSlug={workspaceSlug}
      userId={userId}
      currentUserId={currentUserId}
      {...(initial === undefined ? {} : { initialPage: initial })}
      {...(canDownload ? { download: <ProfileActivityDownload workspaceSlug={workspaceSlug} userId={userId} /> } : {})}
    />
  );
}
