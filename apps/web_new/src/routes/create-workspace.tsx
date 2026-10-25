// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Standalone workspace creation (AUTH-040, AUTH-041). Signed-in-only:
// visitors bounce to the entry with the return path and unfinished
// members to onboarding. The page mounts the shared create form with
// full-page chrome (brand mark home, the signed-in address, a team-size
// dropdown instead of pills); success enters the new workspace directly
// with no invite step. When the instance config disables creation, the
// admin-only screen replaces the form. Written from parity/specs/auth.md.

import { Spinner, ToastHost } from "@pidash/kit";
import { createFileRoute, Link, redirect, useNavigate, useRouter } from "@tanstack/react-router";
import * as React from "react";

import {
  decidePageGate,
  instanceConfigQueryOptions,
  isOnboardedProfile,
  loadGateInputs,
  ONBOARDING_PATH,
  useInstanceConfig,
  useMe,
} from "../core/session/index.js";
import { CreationDisabledScreen } from "../features/auth/components/CreationDisabledScreen.js";
import { TeamSizeDropdown } from "../features/auth/components/TeamSizeDropdown.js";
import { WorkspaceCreateForm } from "../features/auth/components/WorkspaceCreateForm.js";
import { getToastManager, useSessionExpiredRedirect } from "../shared/shell/index.js";

export const Route = createFileRoute("/create-workspace")({
  beforeLoad: async ({ context, location }) => {
    const { me, profile } = await loadGateInputs(context.queryClient, context.apiClient);
    const decision = decidePageGate({
      page: "signed-in-only",
      signedIn: me !== null,
      onboarded: isOnboardedProfile(profile),
      hasPassword: me?.is_password_autoset === false,
      currentPath: location.href,
    });
    if (decision.action === "sign-in") {
      throw redirect({ to: "/", search: { next_path: decision.nextPath } });
    }
    if (decision.action === "onboarding") {
      throw redirect({ href: ONBOARDING_PATH });
    }
    // Prefetch the creation gate; a failed read fails open to the form.
    await context.queryClient.ensureQueryData(instanceConfigQueryOptions(context.apiClient)).catch(() => undefined);
    return {};
  },
  component: function CreateWorkspaceComponent() {
    return <CreateWorkspaceScreen />;
  },
});

export function CreateWorkspaceScreen(): React.ReactElement {
  useSessionExpiredRedirect();
  const router = useRouter();
  const navigate = useNavigate();
  const me = useMe();
  const instanceConfig = useInstanceConfig();
  const account = me.data;

  if (me.isLoading || instanceConfig.isLoading || account === undefined) {
    return (
      <main className="grid min-h-screen place-items-center bg-(--bg)">
        <Spinner label="Loading workspace creation" />
        <ToastHost manager={getToastManager()} />
      </main>
    );
  }

  const creationDisabled = instanceConfig.data?.is_workspace_creation_disabled === true;
  return (
    <main className="flex min-h-screen flex-col bg-(--bg)">
      <header className="flex w-full items-center justify-between gap-(--space-4) px-(--space-6) py-(--space-3)">
        <Link to="/" className="text-emphasis text-(--text)">
          Pi Dash
        </Link>
        <span className="text-caption text-(--text-muted)">{account.email}</span>
      </header>
      <div className="mx-auto w-full max-w-md px-(--space-6) py-(--space-8)">
        {creationDisabled ? (
          <CreationDisabledScreen
            requester={{ firstName: account.first_name, lastName: account.last_name, email: account.email }}
            onBack={() => router.history.back()}
          />
        ) : (
          <WorkspaceCreateForm
            onCreated={(workspace) => navigate({ to: "/$ws", params: { ws: workspace.slug } })}
            renderSizeSelector={(field) => <TeamSizeDropdown {...field} />}
          />
        )}
      </div>
      <ToastHost manager={getToastManager()} />
    </main>
  );
}
