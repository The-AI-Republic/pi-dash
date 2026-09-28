/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";
import { observer } from "mobx-react";
import { useParams, useSearchParams } from "react-router";
// components
import { LogoSpinner } from "@/components/common/logo-spinner";
// hooks
import { useUserSettings } from "@/hooks/store/user";
import { useWorkspace } from "@/hooks/store/use-workspace";
import { useAppRouter } from "@/hooks/use-app-router";
// lib
import { AuthenticationWrapper } from "@/lib/wrappers/authentication-wrapper";

/**
 * Account settings used to live at the global `/settings/profile/:tab`. They now
 * live at `/:workspaceSlug/settings/account/:tab` so they inherit the app shell,
 * which means resolving a workspace before we can redirect — hence a component
 * rather than a `clientLoader`.
 *
 * Handles `/settings`, `/settings/profile`, `/settings/profile/:tab` and any
 * other `/settings/*` leftover.
 */
export const tabFromSplat = (splat: string): string => {
  const segments = splat.split("/").filter(Boolean);
  // Drop the legacy `profile` prefix: `profile/security` -> `security`.
  const rest = segments[0] === "profile" ? segments.slice(1) : segments;
  return rest[0] ?? "general";
};

const SettingsRedirect = observer(function SettingsRedirect() {
  // router
  const router = useAppRouter();
  const params = useParams();
  const [searchParams] = useSearchParams();
  // store hooks
  const { data: currentUserSettings } = useUserSettings();
  const { workspaces } = useWorkspace();
  // derived values
  const tab = tabFromSplat(params["*"] ?? "");
  const query = searchParams.toString();
  const lastWorkspaceSlug =
    currentUserSettings?.workspace?.last_workspace_slug || currentUserSettings?.workspace?.fallback_workspace_slug;
  const workspaceSlug = Object.values(workspaces ?? {}).find((workspace) => workspace.slug === lastWorkspaceSlug)?.slug;

  useEffect(() => {
    if (!currentUserSettings) return;
    // A user with no workspace has nowhere to host account settings.
    if (!workspaceSlug) {
      router.replace("/create-workspace/");
      return;
    }
    router.replace(`/${workspaceSlug}/settings/account/${tab}${query ? `?${query}` : ""}`);
  }, [currentUserSettings, workspaceSlug, tab, query, router]);

  return (
    <div className="grid size-full place-items-center px-4">
      <LogoSpinner />
    </div>
  );
});

export default function SettingsIndexRedirect() {
  return (
    <AuthenticationWrapper>
      <SettingsRedirect />
    </AuthenticationWrapper>
  );
}
