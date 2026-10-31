// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository sync tab (parity/specs/project-settings-part-f.md
// §Repository tab, PSET-050–052). A skeleton while the binding read is
// unresolved; then either the bound card (provider family, the
// repository as an outbound link, the mirroring contract, the fixed
// four-hour cadence with the last success, a failure line when one
// exists, and the admin switch plus confirm-free unbind) or the guidance
// card pointing at General settings. This tab never binds and offers no
// branch or direction choice.

import type { ProjectRepositoryBinding } from "@pidash/api-client";
import { Button, Skeleton, Switch } from "@pidash/kit";
import { ExternalLink } from "lucide-react";
import * as React from "react";

import { useRepositoryStatus, useToggleRepositorySync, useUnbindRepository } from "./api.js";

export const REPOSITORY_CARD_HEADING = "Repository sync";
export const REPOSITORY_BOUND_PREFIX = "Bound to";
export const REPOSITORY_CONTRACT =
  "Mirrored issues are read-only on their title, description and synced comment bodies. Workflow fields stay editable.";
export const REPOSITORY_CADENCE = "Sync issues every 4 hours";
export const REPOSITORY_LAST_ERROR_PREFIX = "Last error:";
export const REPOSITORY_SYNC_LABEL = "Sync issues with this repository";
export const REPOSITORY_UNBIND_LABEL = "Unbind";
export const REPOSITORY_GUIDANCE_BODY =
  "Set the project’s Git repository URL in General Settings and select Bind. Once a repository is bound there, sync can be switched on from this page. Linking lives beside the stored URL so the two can never drift apart.";

const PROVIDER_LABELS: Record<string, string> = {
  github: "GitHub",
  gitlab: "GitLab",
};

/** Provider family shown in the card title (GitHub, GitLab, or a generic fallback). */
export function providerLabel(provider: string): string {
  return PROVIDER_LABELS[provider] ?? "Git provider";
}

const SYNC_TIME_FORMAT = new Intl.DateTimeFormat("en-US", {
  year: "numeric",
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

/** The cadence line, appending the last successful sync when one exists. */
export function cadenceLine(lastSyncedAt: string | null): string {
  if (lastSyncedAt === null) return REPOSITORY_CADENCE;
  return `${REPOSITORY_CADENCE} — last sync ${SYNC_TIME_FORMAT.format(new Date(lastSyncedAt))}`;
}

function BoundCard({
  binding,
  workspaceSlug,
  projectId,
  canManage,
}: {
  binding: ProjectRepositoryBinding;
  workspaceSlug: string;
  projectId: string;
  canManage: boolean;
}): React.ReactElement {
  const sync = useToggleRepositorySync(workspaceSlug, projectId);
  const unbind = useUnbindRepository(workspaceSlug, projectId);
  const busy = sync.isPending || unbind.isPending;
  return (
    <section
      aria-label="Bound repository"
      data-testid="repository-bound-card"
      className="flex flex-col gap-(--space-4) rounded-(--radius-control) border border-(--border) p-(--space-4)"
    >
      <div className="flex items-start justify-between gap-(--space-4)">
        <div className="flex min-w-0 flex-col gap-(--space-1)">
          <h3 className="text-emphasis text-(--text)">{providerLabel(binding.provider)}</h3>
          <p className="text-body text-(--text-muted)">
            {REPOSITORY_BOUND_PREFIX}{" "}
            <a
              href={binding.repository.web_url}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-(--space-1) text-(--accent) underline"
            >
              {binding.repository.full_name}
              <ExternalLink size={12} aria-hidden="true" />
            </a>
            . {REPOSITORY_CONTRACT}
          </p>
        </div>
        <Button
          variant="danger"
          size="small"
          {...(unbind.isPending ? { loading: true } : {})}
          disabled={!canManage || busy}
          onClick={() => unbind.mutate()}
        >
          {REPOSITORY_UNBIND_LABEL}
        </Button>
      </div>

      <div className="flex items-center gap-(--space-3)">
        <Switch
          label={REPOSITORY_SYNC_LABEL}
          checked={binding.is_sync_enabled}
          disabled={!canManage || busy}
          onCheckedChange={(next) => sync.mutate(next)}
        />
        <span className="text-body text-(--text-muted)">{cadenceLine(binding.last_synced_at)}</span>
      </div>

      {binding.last_sync_error === "" ? null : (
        <p className="text-body text-(--danger)">
          {REPOSITORY_LAST_ERROR_PREFIX} {binding.last_sync_error}
        </p>
      )}
    </section>
  );
}

function GuidanceCard(): React.ReactElement {
  return (
    <section
      aria-label="Repository sync"
      data-testid="repository-guidance-card"
      className="flex flex-col gap-(--space-2) rounded-(--radius-control) border border-(--border) p-(--space-4)"
    >
      <h3 className="text-emphasis text-(--text)">{REPOSITORY_CARD_HEADING}</h3>
      <p className="text-body text-(--text-muted)">{REPOSITORY_GUIDANCE_BODY}</p>
    </section>
  );
}

export interface RepositorySyncCardProps {
  workspaceSlug: string;
  projectId: string;
  canManage: boolean;
}

/** Skeleton the binding read shows while unresolved (PSET-050/061). */
function BindingSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading repository binding" className="flex flex-col gap-(--space-2) p-(--space-4)">
      <Skeleton width="160px" height="20px" />
      <Skeleton height="32px" />
    </div>
  );
}

export function RepositorySyncCard({
  workspaceSlug,
  projectId,
  canManage,
}: RepositorySyncCardProps): React.ReactElement {
  const status = useRepositoryStatus(workspaceSlug, projectId);

  if (status.isPending) return <BindingSkeleton />;

  if (status.isError) {
    return (
      <p role="alert" className="text-body text-(--danger)">
        The repository status could not be loaded. Reload the page to try again.
      </p>
    );
  }

  if (!status.data.bound) return <GuidanceCard />;
  return <BoundCard binding={status.data} workspaceSlug={workspaceSlug} projectId={projectId} canManage={canManage} />;
}
