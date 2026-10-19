// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Webhook detail (parity/specs/workspace-settings.md §21-§24,
// WSET-022/023/024). One page owns the whole lifecycle of a single
// webhook: the read (spinner, missing-or-deleted explanation for an
// unknown id, retryable failure), the edit form (target address, event
// selection derived from the stored flags, active switch), the secret
// block (always present; actions only while a freshly issued key sits in
// memory), regeneration with its fresh backup, and the collapsed danger
// zone whose confirmation deletes the webhook and returns to the list.

import { Button, Dialog, Input, Spinner, Switch } from "@pidash/kit";
import { useNavigate } from "@tanstack/react-router";
import * as React from "react";

import type { CreatedWorkspaceWebhook, WorkspaceWebhook } from "@pidash/api-client";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { nextWorkspaceSettingsDialogOpen } from "../lib/dialog.js";
import {
  WEBHOOK_DETAIL_ERROR_BODY,
  WEBHOOK_DETAIL_ERROR_HEADING,
  WebhookDetailMissing,
} from "../components/SettingsWebhookDetailOutlet.js";
import { resolveWebhookDetailStatus, type WorkspaceWebhookReadState } from "../lib/webhookRoute.js";
import { useDeleteWebhook, useRegenerateWebhookSecret, useUpdateWebhook, useWebhookDetail } from "./api.js";
import { downloadWebhookBackup, webhookBackup } from "./backup.js";
import { EventPicker, type WebhookEventSelection } from "./EventPicker.js";
import {
  eventSelectionFor,
  normalizeWebhookUrl,
  submittedFlags,
  WEBHOOK_URL_REQUIRED,
  type WebhookEventFlags,
} from "./events.js";
import { SecretKey } from "./SecretKey.js";

export const DETAIL_HEADING = "Webhook details";
export const DETAIL_URL_ERROR = WEBHOOK_URL_REQUIRED;
export const UPDATE_LABEL = "Update";
export const UPDATING_LABEL = "Updating";
export const WEBHOOK_UPDATED_NOTICE = "Webhook updated";
export const WEBHOOK_UPDATE_FAILED_NOTICE = "Error";
export const WEBHOOK_UPDATE_FAILED_BODY = "The webhook could not be updated. Check the target address and try again.";
export const SECRET_BLOCK_TITLE = "Secret key";
export const SECRET_BLOCK_BODY =
  "The secret signs every delivery. You can rotate it at any time; the previous key stops working immediately.";
export const REGENERATE_LABEL = "Re-generate key";
export const REGENERATING_LABEL = "Re-generating…";
export const WEBHOOK_REGENERATED_NOTICE = "Webhook secret regenerated";
export const WEBHOOK_REGENERATE_FAILED_NOTICE = "Error";
export const WEBHOOK_REGENERATE_FAILED_BODY = "The secret key could not be regenerated.";
export const DANGER_ZONE_TITLE = "Danger zone";
export const DANGER_ZONE_BODY =
  "Deleting a webhook is permanent. Once a webhook is deleted it stops receiving deliveries, and its secret key stops working; the deliveries already sent are not affected.";
export const DELETE_TRIGGER_LABEL = "Delete webhook";
export const DELETE_DIALOG_TITLE = "Delete webhook";
export const DELETE_DIALOG_BODY =
  "This action cannot be undone. The webhook will be removed and its secret key retired immediately.";
export const WEBHOOK_DELETED_NOTICE = "Webhook deleted";
export const WEBHOOK_DELETE_FAILED_NOTICE = "The webhook could not be deleted";
export const WEBHOOK_DELETE_FAILED_BODY = "It may already have been deleted. Reload the page to see the latest state.";
export const ACTIVE_LABEL = "Active";
export const ACTIVE_HINT = "Inactive webhooks keep their secret but receive no deliveries.";

export interface WebhookDetailPageProps {
  workspaceSlug: string;
  webhookId: string;
}

export function WebhookDetailPage({ workspaceSlug, webhookId }: WebhookDetailPageProps): React.ReactElement {
  const detail = useWebhookDetail(workspaceSlug, webhookId);
  const readState: WorkspaceWebhookReadState<WorkspaceWebhook> = {
    status: detail.status === "success" || detail.status === "error" ? detail.status : "pending",
    ...(detail.data === undefined ? {} : { data: detail.data }),
    ...(detail.error === null ? {} : { error: detail.error }),
  };
  const status = resolveWebhookDetailStatus(readState);
  if (status === "loading") return <WebhookDetailLoader />;
  if (status === "missing") return <WebhookDetailMissing />;
  if (status === "error") return <WebhookDetailError />;
  // "ready" is only reachable with a row in hand (resolveWebhookDetailStatus).
  const webhook = readState.data as WorkspaceWebhook;
  // Remount on a different id so no draft or key survives a navigation.
  return <WebhookDetailForm key={webhookId} workspaceSlug={workspaceSlug} webhook={webhook} />;
}

function WebhookDetailLoader(): React.ReactElement {
  return (
    <div
      role="status"
      aria-label="Loading webhook details"
      data-testid="workspace-settings-webhook-detail-loading"
      className="flex flex-col gap-(--space-4)"
    >
      <div className="flex items-center gap-(--space-2) text-(--text-muted)">
        <Spinner size="small" label="Loading webhook details" />
        <span className="text-meta">Loading webhook details…</span>
      </div>
      <div aria-hidden="true" className="flex flex-col gap-(--space-3)">
        <span className="block h-8 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
        <span className="block h-24 w-full animate-pulse rounded-(--radius-control) bg-(--subtle)" />
      </div>
    </div>
  );
}

function WebhookDetailError(): React.ReactElement {
  return (
    <section
      role="alert"
      data-testid="workspace-settings-webhook-detail-error"
      className="flex flex-col items-start gap-(--space-2) rounded-(--radius-control) border border-(--border) px-(--space-6) py-(--space-6)"
    >
      <h2 className="text-h3 text-(--text)">{WEBHOOK_DETAIL_ERROR_HEADING}</h2>
      <p className="text-body text-(--text-muted)">{WEBHOOK_DETAIL_ERROR_BODY}</p>
    </section>
  );
}

interface WebhookDetailFormProps {
  workspaceSlug: string;
  webhook: WorkspaceWebhook;
}

function WebhookDetailForm({ workspaceSlug, webhook }: WebhookDetailFormProps): React.ReactElement {
  const navigate = useNavigate();
  const update = useUpdateWebhook(workspaceSlug);
  const regenerate = useRegenerateWebhookSecret(workspaceSlug);
  const remove = useDeleteWebhook(workspaceSlug);

  const [url, setUrl] = React.useState(webhook.url);
  const [active, setActive] = React.useState(webhook.is_active);
  const [selection, setSelection] = React.useState<WebhookEventSelection>(() => eventSelectionFor(webhook));
  const [urlError, setUrlError] = React.useState<string | null>(null);
  const [secret, setSecret] = React.useState<string | null>(null);
  const [dangerOpen, setDangerOpen] = React.useState(false);
  const [deleteOpen, setDeleteOpen] = React.useState(false);

  // The server is the source of truth after a save or a rotation.
  React.useEffect(() => {
    setUrl(webhook.url);
    setActive(webhook.is_active);
    setSelection(eventSelectionFor(webhook));
  }, [webhook]);

  const submit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    const address = normalizeWebhookUrl(url);
    if (address === "") {
      setUrlError(DETAIL_URL_ERROR);
      return;
    }
    setUrlError(null);
    update.mutate(
      { webhookId: webhook.id, input: { url: address, is_active: active, ...submittedFlags(selection) } },
      {
        onSuccess: () => {
          notifySuccess(WEBHOOK_UPDATED_NOTICE);
        },
        onError: () => {
          notifyError(WEBHOOK_UPDATE_FAILED_NOTICE, WEBHOOK_UPDATE_FAILED_BODY);
        },
      }
    );
  };

  const rotate = (): void => {
    regenerate.mutate(webhook.id, {
      onSuccess: (issued: CreatedWorkspaceWebhook) => {
        notifySuccess(WEBHOOK_REGENERATED_NOTICE);
        // A fresh backup with the new key on every success (NEWFRONT-419).
        downloadWebhookBackup(webhookBackup(issued, workspaceSlug, issued.secret_key, Date.now()));
        setSecret(issued.secret_key);
      },
      onError: () => {
        notifyError(WEBHOOK_REGENERATE_FAILED_NOTICE, WEBHOOK_REGENERATE_FAILED_BODY);
      },
    });
  };

  const confirmDelete = (): void => {
    remove.mutate(webhook.id, {
      onSuccess: () => {
        setDeleteOpen(false);
        notifySuccess(WEBHOOK_DELETED_NOTICE);
        void navigate({ to: "/$ws/settings/webhooks", params: { ws: workspaceSlug } });
      },
      onError: () => {
        setDeleteOpen(false);
        notifyError(WEBHOOK_DELETE_FAILED_NOTICE, WEBHOOK_DELETE_FAILED_BODY);
      },
    });
  };

  const flags: WebhookEventFlags = selection.flags;

  return (
    <div className="flex max-w-2xl flex-col gap-(--space-8)">
      <h2 className="text-h3 text-(--text)">{DETAIL_HEADING}</h2>
      <form className="flex flex-col gap-(--space-6)" onSubmit={submit} noValidate>
        <Input
          label="Target URL"
          placeholder="https://example.com/post"
          value={url}
          required
          onChange={(event) => {
            setUrl(event.target.value);
            if (urlError !== null) setUrlError(null);
          }}
          {...(urlError === null ? {} : { error: urlError })}
        />
        <div className="flex items-start justify-between gap-(--space-6)">
          <div className="flex flex-col gap-(--space-1)">
            <span className="text-body font-medium text-(--text)">{ACTIVE_LABEL}</span>
            <span className="text-meta text-(--text-muted)">{ACTIVE_HINT}</span>
          </div>
          <Switch aria-label={ACTIVE_LABEL} checked={active} onCheckedChange={setActive} />
        </div>
        <EventPicker value={{ mode: selection.mode, flags }} onChange={setSelection} />
        <div className="flex justify-end">
          <Button variant="primary" type="submit" loading={update.isPending}>
            {update.isPending ? UPDATING_LABEL : UPDATE_LABEL}
          </Button>
        </div>
      </form>

      <section
        aria-labelledby="webhook-secret-heading"
        className="flex flex-col gap-(--space-4) rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-4)"
      >
        <div className="flex flex-col gap-(--space-1)">
          <h3 id="webhook-secret-heading" className="text-emphasis font-medium text-(--text)">
            {SECRET_BLOCK_TITLE}
          </h3>
          <p className="text-meta text-(--text-muted)">{SECRET_BLOCK_BODY}</p>
        </div>
        {secret === null ? null : <SecretKey secret={secret} />}
        <div className="flex justify-end">
          <Button onClick={rotate} loading={regenerate.isPending}>
            {regenerate.isPending ? REGENERATING_LABEL : REGENERATE_LABEL}
          </Button>
        </div>
      </section>

      <section aria-labelledby="webhook-danger-heading" className="flex flex-col gap-(--space-3)">
        <button
          type="button"
          id="webhook-danger-heading"
          aria-expanded={dangerOpen}
          onClick={() => setDangerOpen((open) => !open)}
          className="text-emphasis flex cursor-pointer items-center justify-between gap-(--space-4) rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-3) text-left font-medium text-(--text) hover:bg-(--subtle) focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-(--focus-ring-color) focus-visible:outline-solid"
        >
          {DANGER_ZONE_TITLE}
          <span aria-hidden="true" className="text-(--text-muted)">
            {dangerOpen ? "−" : "+"}
          </span>
        </button>
        {dangerOpen ? (
          <div className="flex flex-col gap-(--space-3) rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-4)">
            <p className="text-body text-(--text-muted)">{DANGER_ZONE_BODY}</p>
            <div>
              <Button variant="danger" onClick={() => setDeleteOpen(true)}>
                {DELETE_TRIGGER_LABEL}
              </Button>
            </div>
          </div>
        ) : null}
      </section>

      <Dialog
        open={deleteOpen}
        onOpenChange={(next, details) => {
          setDeleteOpen(
            nextWorkspaceSettingsDialogOpen(deleteOpen, { open: next, reason: details.reason }, { reveal: false })
          );
        }}
        title={DELETE_DIALOG_TITLE}
        actions={
          <>
            <Button onClick={() => setDeleteOpen(false)}>Cancel</Button>
            <Button variant="danger" loading={remove.isPending} onClick={confirmDelete}>
              Delete
            </Button>
          </>
        }
      >
        <p className="text-body text-(--text-muted)">{DELETE_DIALOG_BODY}</p>
      </Dialog>
    </div>
  );
}
