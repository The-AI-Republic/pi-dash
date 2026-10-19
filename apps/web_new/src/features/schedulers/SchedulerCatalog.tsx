// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace scheduler catalog from parity/specs/agents.md §1 (AGT-001–005).

import type { Scheduler } from "@pidash/api-client";
import { Badge, Button, Skeleton } from "@pidash/kit";
import { Download, Pencil, Plus, Trash2 } from "lucide-react";
import * as React from "react";

import { usePermissions, useWorkspace } from "../../core/session/index.js";
import { useDocumentHead } from "../../shared/shell/index.js";
import { useSchedulers } from "./api.js";
import { SchedulerDefinitionDialog } from "./SchedulerDefinitionDialog.js";
import { SchedulerDeleteDialog } from "./SchedulerDeleteDialog.js";
import { BulkInstallDialog } from "./BulkInstallDialog.js";

export interface SchedulerCatalogProps {
  workspaceSlug: string;
}

export function schedulerSourceLabel(source: Scheduler["source"]): string {
  return source === "builtin" ? "Built in" : "Manifest";
}

export function formatSchedulerUpdatedAt(value: string): string {
  const timestamp = new Date(value);
  if (Number.isNaN(timestamp.getTime())) return "Unknown";
  return new Intl.DateTimeFormat("en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(timestamp);
}

function CatalogSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading schedulers" className="flex flex-col gap-(--space-3) p-(--space-6)">
      {[0, 1, 2].map((row) => (
        <div key={row} className="flex items-center gap-(--space-6)">
          <Skeleton width="180px" height="16px" />
          <Skeleton width="100px" height="16px" />
          <Skeleton width="70px" height="16px" />
        </div>
      ))}
    </div>
  );
}

export function SchedulerCatalog({ workspaceSlug }: SchedulerCatalogProps): React.ReactElement {
  const { workspace } = useWorkspace(workspaceSlug);
  const permissions = usePermissions(workspaceSlug);
  const schedulers = useSchedulers(workspaceSlug);
  const [editor, setEditor] = React.useState<Scheduler | "create" | null>(null);
  const [deleting, setDeleting] = React.useState<Scheduler | null>(null);
  const [installing, setInstalling] = React.useState<Scheduler | null>(null);
  useDocumentHead({
    title: `${workspace?.name ?? workspaceSlug} · Schedulers`,
    robots: "noindex, nofollow",
  });

  return (
    <section aria-labelledby="scheduler-catalog-heading" className="flex h-full flex-col bg-(--bg)">
      <header className="flex items-center justify-between gap-(--space-6) border-b border-(--border) px-(--space-8) py-(--space-6)">
        <div>
          <h1 id="scheduler-catalog-heading" className="text-h2 text-(--text)">
            Schedulers
          </h1>
          <p className="text-body text-(--text-muted)">Reusable instructions that projects can run on a schedule.</p>
        </div>
        {permissions.isAdmin ? (
          <Button variant="primary" onClick={() => setEditor("create")}>
            <Plus size={16} strokeWidth={1.5} aria-hidden="true" />
            New scheduler
          </Button>
        ) : null}
      </header>

      {schedulers.isPending || permissions.isLoading ? <CatalogSkeleton /> : null}
      {schedulers.isError && !permissions.isLoading ? (
        <div role="alert" className="flex flex-col items-start gap-(--space-4) p-(--space-8)">
          <h2 className="text-h3 text-(--text)">Schedulers could not be loaded</h2>
          <p className="text-body text-(--text-muted)">{schedulers.error.message}</p>
          <Button variant="primary" onClick={() => void schedulers.refetch()}>
            Try again
          </Button>
        </div>
      ) : null}
      {schedulers.isSuccess && !permissions.isLoading ? (
        <div className="overflow-auto p-(--space-8)">
          <table
            className="text-body w-full border-separate border-spacing-0 text-left"
            aria-label="Scheduler definitions"
          >
            <thead>
              <tr className="text-meta text-(--text-muted)">
                <th className="border-b border-(--border) px-(--space-4) py-(--space-3) font-medium">Name</th>
                <th className="border-b border-(--border) px-(--space-4) py-(--space-3) font-medium">Origin</th>
                <th className="border-b border-(--border) px-(--space-4) py-(--space-3) font-medium">Installs</th>
                <th className="border-b border-(--border) px-(--space-4) py-(--space-3) font-medium">Status</th>
                <th className="border-b border-(--border) px-(--space-4) py-(--space-3) font-medium">Updated</th>
                {permissions.isAdmin ? (
                  <th className="border-b border-(--border) px-(--space-4) py-(--space-3) text-right font-medium">
                    Actions
                  </th>
                ) : null}
              </tr>
            </thead>
            <tbody>
              {schedulers.data.length === 0 ? (
                <tr>
                  <td colSpan={permissions.isAdmin ? 6 : 5} className="px-(--space-4) py-(--space-12) text-center">
                    <p className="font-medium text-(--text)">No scheduler definitions yet</p>
                    <p className="text-meta text-(--text-muted)">
                      {permissions.isAdmin
                        ? "Create one to make repeatable work available to projects."
                        : "A workspace admin can add reusable scheduled work here."}
                    </p>
                  </td>
                </tr>
              ) : (
                schedulers.data.map((scheduler) => (
                  <tr key={scheduler.id} data-testid="scheduler-row" className="hover:bg-(--subtle)">
                    <td className="border-b border-(--border) px-(--space-4) py-(--space-4)">
                      <p className="font-medium text-(--text)">{scheduler.name}</p>
                      <p className="text-meta font-mono text-(--text-muted)">{scheduler.slug}</p>
                    </td>
                    <td className="border-b border-(--border) px-(--space-4) py-(--space-4)">
                      <Badge>{schedulerSourceLabel(scheduler.source)}</Badge>
                    </td>
                    <td className="border-b border-(--border) px-(--space-4) py-(--space-4) text-(--text)">
                      {scheduler.active_binding_count}
                    </td>
                    <td className="border-b border-(--border) px-(--space-4) py-(--space-4)">
                      <Badge dot tone={scheduler.is_enabled ? "success" : "neutral"}>
                        {scheduler.is_enabled ? "Enabled" : "Disabled"}
                      </Badge>
                    </td>
                    <td className="border-b border-(--border) px-(--space-4) py-(--space-4) text-(--text-muted)">
                      <time dateTime={scheduler.updated_at}>{formatSchedulerUpdatedAt(scheduler.updated_at)}</time>
                    </td>
                    {permissions.isAdmin ? (
                      <td className="border-b border-(--border) px-(--space-4) py-(--space-4)">
                        <div className="flex justify-end gap-(--space-2)">
                          {scheduler.is_enabled ? (
                            <Button size="small" variant="ghost" onClick={() => setInstalling(scheduler)}>
                              <Download size={14} strokeWidth={1.5} aria-hidden="true" />
                              Install
                            </Button>
                          ) : null}
                          <Button size="small" variant="ghost" onClick={() => setEditor(scheduler)}>
                            <Pencil size={14} strokeWidth={1.5} aria-hidden="true" />
                            Edit
                          </Button>
                          <Button size="small" variant="ghost" onClick={() => setDeleting(scheduler)}>
                            <Trash2 size={14} strokeWidth={1.5} aria-hidden="true" />
                            Delete
                          </Button>
                        </div>
                      </td>
                    ) : null}
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      ) : null}

      <SchedulerDefinitionDialog
        workspaceSlug={workspaceSlug}
        open={editor !== null}
        scheduler={editor === "create" ? null : editor}
        onClose={() => setEditor(null)}
      />
      <SchedulerDeleteDialog workspaceSlug={workspaceSlug} scheduler={deleting} onClose={() => setDeleting(null)} />
      <BulkInstallDialog workspaceSlug={workspaceSlug} scheduler={installing} onClose={() => setInstalling(null)} />
    </section>
  );
}
