// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhooks list (parity/specs/workspace-settings.md §18/§24,
// WSET-019/041). Admin-only by the shell's gate; this page owns the
// heading and its Add control, the loading skeleton, the illustrated
// empty state (same creation dialog), and one row per webhook: the row
// links to the detail page and carries the active switch. Each row's
// switch writes `is_active` alone and shows the stored answer.

import { Button, EmptyState, Spinner, Switch } from "@pidash/kit";
import { Link } from "@tanstack/react-router";
import * as React from "react";

import type { WorkspaceWebhook } from "@pidash/api-client";

import { useSetWebhookActive, useWebhooksList } from "./api.js";
import { CreateWebhookDialog } from "./CreateWebhookDialog.js";

export const WEBHOOKS_HEADING = "Webhooks";
export const WEBHOOKS_DESCRIPTION =
  "Send deliveries to other tools when projects, cycles, work items, modules or comments change.";
export const WEBHOOKS_ADD_LABEL = "Add webhook";
export const WEBHOOKS_EMPTY_TITLE = "No Webhook added yet";
export const WEBHOOKS_EMPTY_BODY =
  "Create a webhook to broadcast workspace events to an address you control. The secret key is shown once, right after creation.";
export const WEBHOOKS_LOAD_FAILED = "Couldn't load the webhooks.";
export const WEBHOOKS_ROW_SWITCH_LABEL = "Webhook active";

/** Address of a webhook's detail page inside the settings shell. */
export function webhookDetailHref(workspaceSlug: string, webhookId: string): string {
  return `/${workspaceSlug}/settings/webhooks/${webhookId}`;
}

export interface WebhooksListPageProps {
  workspaceSlug: string;
}

export function WebhooksListPage({ workspaceSlug }: WebhooksListPageProps): React.ReactElement {
  const list = useWebhooksList(workspaceSlug);
  const [creating, setCreating] = React.useState(false);
  const rows = list.data ?? [];
  const empty = !list.isPending && !list.isError && rows.length === 0;

  return (
    <div className="flex flex-col gap-(--space-6)">
      <header className="flex items-start justify-between gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <h2 className="text-h3 text-(--text)">{WEBHOOKS_HEADING}</h2>
          <p className="text-body max-w-xl text-(--text-muted)">{WEBHOOKS_DESCRIPTION}</p>
        </div>
        {list.isPending ? null : (
          <Button variant="primary" onClick={() => setCreating(true)}>
            {WEBHOOKS_ADD_LABEL}
          </Button>
        )}
      </header>
      {list.isPending ? <WebhooksSkeleton /> : null}
      {list.isError ? (
        <p role="alert" className="text-body text-(--danger)">
          {WEBHOOKS_LOAD_FAILED}
        </p>
      ) : null}
      {empty ? (
        <EmptyState
          variant="detailed"
          art={{ category: "general", type: "empty" }}
          title={WEBHOOKS_EMPTY_TITLE}
          description={WEBHOOKS_EMPTY_BODY}
          primaryAction={{ label: WEBHOOKS_ADD_LABEL, onSelect: () => setCreating(true) }}
        />
      ) : null}
      {rows.length > 0 ? (
        <ul
          aria-label="Webhooks"
          className="flex flex-col divide-y divide-(--border) rounded-(--radius-control) border border-(--border)"
        >
          {rows.map((row) => (
            <WebhookRow key={row.id} workspaceSlug={workspaceSlug} webhook={row} />
          ))}
        </ul>
      ) : null}
      <CreateWebhookDialog workspaceSlug={workspaceSlug} open={creating} onOpenChange={setCreating} />
    </div>
  );
}

function WebhooksSkeleton(): React.ReactElement {
  return (
    <div role="status" aria-label="Loading webhooks" className="flex flex-col gap-(--space-4)">
      <div className="flex items-center gap-(--space-2) text-(--text-muted)">
        <Spinner size="small" label="Loading webhooks" />
        <span className="text-meta">Loading webhooks…</span>
      </div>
      <div
        aria-hidden="true"
        className="flex flex-col divide-y divide-(--border) rounded-(--radius-control) border border-(--border)"
      >
        {[0, 1, 2].map((index) => (
          <div key={index} className="flex items-center justify-between gap-(--space-4) px-(--space-4) py-(--space-3)">
            <span className="block h-3 w-1/2 animate-pulse rounded-(--radius-control) bg-(--subtle)" />
            <span className="block h-5 w-9 animate-pulse rounded-full bg-(--subtle)" />
          </div>
        ))}
      </div>
    </div>
  );
}

export interface WebhookRowProps {
  workspaceSlug: string;
  webhook: WorkspaceWebhook;
}

function WebhookRow({ workspaceSlug, webhook }: WebhookRowProps): React.ReactElement {
  const setActive = useSetWebhookActive(workspaceSlug);
  return (
    <li className="flex items-center gap-(--space-4) px-(--space-4) py-(--space-2)">
      <Link
        to="/$ws/settings/webhooks/$webhookId"
        params={{ ws: workspaceSlug, webhookId: webhook.id }}
        className="text-body flex min-w-0 flex-1 items-center justify-between gap-(--space-4) rounded-(--radius-control) text-(--text) hover:text-(--accent) focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-(--focus-ring-color) focus-visible:outline-solid"
      >
        <span className="truncate">{webhook.url}</span>
        <span
          onClick={(event) => {
            // The switch sits inside the row link: stop the bubble before
            // the link navigates. The switch itself still toggles (its own
            // click handler runs first, on the switch element).
            event.stopPropagation();
          }}
        >
          <Switch
            aria-label={WEBHOOKS_ROW_SWITCH_LABEL}
            checked={webhook.is_active}
            disabled={setActive.isPending}
            onCheckedChange={(checked) => {
              setActive.mutate({ webhookId: webhook.id, input: { is_active: checked } });
            }}
          />
        </span>
      </Link>
    </li>
  );
}
