// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace-settings general page (parity/specs/workspace-settings.md
// §3–§6; WSET-003–006). Composes the identity block (logo + copyable
// address), the details form and — for admins only — the delete danger
// zone. It renders inside the NEWFRONT-534 shell, which owns the section
// frame and the role gate; this page repeats only the field-level rule
// that members see a dimmed, inert page. Reads share the frame's details
// key and follow the mount-only policy (§38).

import { Skeleton } from "@pidash/kit";
import { useQuery } from "@tanstack/react-query";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { usePermissions, useMeSettings, useWorkspaces } from "../../../core/session/index.js";
import { notifySuccess, resolveFileUrl } from "../../../shared/shell/index.js";
import { workspaceDetailsQueryOptions } from "./api/details.js";
import { URL_COPIED_TITLE } from "./api/mutations.js";
import { DeleteWorkspaceSection } from "./components/DeleteWorkspaceSection.js";
import { WorkspaceDetailsForm } from "./components/WorkspaceDetailsForm.js";
import { WorkspaceIdentityCard } from "./components/WorkspaceIdentityCard.js";
import { WorkspaceLogoDialog } from "./components/WorkspaceLogoDialog.js";
import { postDeletePath, workspaceAddress } from "./rules.js";

export interface WorkspaceGeneralPageProps {
  workspaceSlug: string;
  onNavigate?: ((path: string) => void) | undefined;
}

/** Copy the full workspace address; a refused clipboard stays silent (§5). */
export async function copyWorkspaceAddress(
  address: string,
  clipboard: Pick<Clipboard, "writeText"> | undefined,
  confirm: () => void
): Promise<void> {
  if (clipboard === undefined) return;
  try {
    await clipboard.writeText(address);
    confirm();
  } catch {
    // Silent by design: no notice, no page disturbance.
  }
}

export function WorkspaceGeneralPage({ workspaceSlug, onNavigate }: WorkspaceGeneralPageProps): React.ReactElement {
  const client = getAppClient();
  const permissions = usePermissions(workspaceSlug);
  const details = useQuery(workspaceDetailsQueryOptions(client, workspaceSlug));
  const settings = useMeSettings();
  const workspaces = useWorkspaces();
  const [logoOpen, setLogoOpen] = React.useState(false);
  const canManage = permissions.isAdmin;

  const stored = details.data;
  // The address a successful delete lands on: the last (then fallback)
  // workspace when it survives, else workspace creation (§6).
  const destination = postDeletePath({
    remainingSlugs: (workspaces.data ?? []).map((entry) => entry.slug).filter((slug) => slug !== workspaceSlug),
    lastWorkspaceSlug: settings.data?.workspace.last_workspace_slug ?? null,
    fallbackWorkspaceSlug: settings.data?.workspace.fallback_workspace_slug ?? null,
    creationPath: "/create-workspace",
  });

  if (stored === undefined) {
    return (
      <div data-testid="workspace-general-loading" className="flex flex-col gap-(--space-4)">
        <Skeleton className="size-14 rounded-(--radius-control)" />
        <Skeleton className="h-6 w-56" />
        <Skeleton className="h-(--control-height) w-full max-w-md" />
      </div>
    );
  }

  const logoSrc = resolveFileUrl(stored.logo_url, client.baseUrl);

  return (
    <div data-testid="workspace-general-page" className="flex flex-col gap-(--space-8)">
      <WorkspaceIdentityCard
        workspaceSlug={workspaceSlug}
        workspaceName={stored.name}
        logoSrc={logoSrc}
        canManage={canManage}
        onOpenLogo={() => setLogoOpen(true)}
        onCopyAddress={() => {
          void copyWorkspaceAddress(workspaceAddress(window.location.origin, workspaceSlug), navigator.clipboard, () =>
            notifySuccess(URL_COPIED_TITLE)
          );
        }}
      />
      <WorkspaceDetailsForm
        workspaceSlug={workspaceSlug}
        canManage={canManage}
        stored={{
          name: stored.name,
          organizationSize: stored.organization_size,
          timezone: stored.timezone,
        }}
      />
      {canManage ? (
        <DeleteWorkspaceSection
          workspaceSlug={workspaceSlug}
          workspaceName={stored.name}
          destination={destination}
          onNavigate={(path) => {
            onNavigate?.(path);
          }}
        />
      ) : null}
      {canManage ? (
        <WorkspaceLogoDialog
          workspaceSlug={workspaceSlug}
          workspaceId={stored.id}
          logoUrl={stored.logo_url}
          open={logoOpen}
          onOpenChange={setLogoOpen}
        />
      ) : null}
    </div>
  );
}
