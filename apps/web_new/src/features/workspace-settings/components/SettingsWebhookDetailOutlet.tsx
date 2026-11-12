// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook detail outlet (parity/specs/workspace-settings.md §36,
// WSET-036; the intended behavior behind NEWFRONT-417). The webhooks
// slice registers the editable body. Until it does, the shell still
// resolves the address: a while the read runs, and a plain
// missing-or-deleted notice for an unknown id instead of an endless
// spinner. A 404 read of the detail endpoint is what identifies the id,
// so the route never claims a webhook exists before it does.

import { useQuery } from "@tanstack/react-query";
import { LoaderCircle, Webhook } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { useWorkspaceWebhookDetailPage } from "../lib/registry.js";
import { resolveWebhookDetailStatus } from "../lib/webhookRoute.js";
import { SettingsSectionPlaceholder } from "./SettingsSectionPlaceholder.js";
import { workspaceWebhookPresenceOptions } from "./webhookPresenceQuery.js";

export const WEBHOOK_DETAIL_MISSING_HEADING = "Webhook not found";
export const WEBHOOK_DETAIL_MISSING_BODY =
  "This webhook no longer exists. It may have been deleted, or the link may point at the wrong workspace.";

export interface SettingsWebhookDetailOutletProps {
  workspaceSlug: string;
  webhookId: string;
}

export function SettingsWebhookDetailOutlet({
  workspaceSlug,
  webhookId,
}: SettingsWebhookDetailOutletProps): React.ReactElement {
  const Page = useWorkspaceWebhookDetailPage();
  // A registered page owns its own read; the shell only checks the
  // address while it has no body to show.
  const presence = useQuery({
    ...workspaceWebhookPresenceOptions(getAppClient(), workspaceSlug, webhookId),
    enabled: Page === null,
  });
  const status = Page === null ? resolveWebhookDetailStatus(presence) : "ready";
  if (status === "ready") {
    return Page === null ? (
      <SettingsSectionPlaceholder />
    ) : (
      <Page workspaceSlug={workspaceSlug} webhookId={webhookId} />
    );
  }
  if (status === "loading") return <WebhookDetailLoading />;
  if (status === "missing") return <WebhookDetailMissing />;
  return <WebhookDetailError />;
}

function WebhookDetailLoading(): React.ReactElement {
  return (
    <div
      role="status"
      aria-live="polite"
      data-testid="workspace-settings-webhook-loading"
      className="text-body flex items-center gap-(--space-2) text-(--text-muted)"
    >
      <LoaderCircle size={16} strokeWidth={1.5} aria-hidden="true" className="animate-spin" />
      Loading webhook…
    </div>
  );
}

export function WebhookDetailMissing(): React.ReactElement {
  return (
    <section
      data-testid="workspace-settings-webhook-missing"
      className="flex flex-col items-start gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-6) py-(--space-6)"
    >
      <span className="flex size-10 items-center justify-center rounded-full bg-(--subtle) text-(--text-muted)">
        <Webhook size={18} strokeWidth={1.5} aria-hidden="true" />
      </span>
      <h2 className="text-h3 text-(--text)">{WEBHOOK_DETAIL_MISSING_HEADING}</h2>
      <p className="text-body text-(--text-muted)">{WEBHOOK_DETAIL_MISSING_BODY}</p>
    </section>
  );
}

export const WEBHOOK_DETAIL_ERROR_HEADING = "That webhook could not be loaded";
export const WEBHOOK_DETAIL_ERROR_BODY = "Reload the page to try again.";

export function WebhookDetailError(): React.ReactElement {
  return (
    <section
      role="alert"
      data-testid="workspace-settings-webhook-error"
      className="flex flex-col items-start gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-6) py-(--space-6)"
    >
      <h2 className="text-h3 text-(--text)">{WEBHOOK_DETAIL_ERROR_HEADING}</h2>
      <p className="text-body text-(--text-muted)">{WEBHOOK_DETAIL_ERROR_BODY}</p>
    </section>
  );
}
