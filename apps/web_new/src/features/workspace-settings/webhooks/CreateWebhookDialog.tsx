// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Create-a-webhook dialog (parity/specs/workspace-settings.md §19/§20,
// WSET-020/021). Two steps in one dialog: the form (target URL plus the
// event selection) and, once the server answers, the one-time key. The
// form step dismisses on Escape, Cancel or the close control without a
// write; the reveal step ignores those dismissals and only closes through
// its own button, which also forgets the key (§20).

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import type { CreatedWorkspaceWebhook } from "@pidash/api-client";

import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { nextWorkspaceSettingsDialogOpen } from "../lib/dialog.js";
import { useCreateWebhook } from "./api.js";
import { downloadWebhookBackup, webhookBackup } from "./backup.js";
import { EventPicker, type WebhookEventSelection } from "./EventPicker.js";
import { initialEventSelection, normalizeWebhookUrl, submittedFlags, WEBHOOK_URL_REQUIRED } from "./events.js";
import { SecretKey } from "./SecretKey.js";

export const CREATE_WEBHOOK_TITLE = "Create webhook";
export const KEY_CREATED_TITLE = "Key created";
export const WEBHOOK_CREATED_NOTICE = "Webhook created";
export const WEBHOOK_NOT_CREATED_NOTICE = "Webhook not created";
export const WEBHOOK_NOT_CREATED_BODY = "The webhook could not be saved. Check the target address and try again.";
export const KEY_CREATED_EXPLAINER =
  "Copy this key now and keep the backup file safe. The key cannot be shown again after you hit Close; a backup file with the key has been downloaded to your computer.";

export interface CreateWebhookDialogProps {
  workspaceSlug: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Fires once a webhook exists, so the list can refresh its rows. */
  onCreated?: ((webhook: CreatedWorkspaceWebhook) => void) | undefined;
}

export function CreateWebhookDialog({
  workspaceSlug,
  open,
  onOpenChange,
  onCreated,
}: CreateWebhookDialogProps): React.ReactElement {
  const [issued, setIssued] = React.useState<{ webhook: CreatedWorkspaceWebhook; secret: string } | null>(null);
  // Every close starts a new session. A write is bound to the session it
  // was submitted in, so an answer that lands after Cancel or Escape is
  // stale: it must not reveal a key, notify, or download a backup (§19/§20).
  const session = React.useRef(0);
  const writeSession = React.useRef<number | null>(null);
  // The hook hands the whole answer over before it resolves; the key is
  // kept here, outside the mutation cache, so closing is the only place
  // that can clear it (§20).
  const create = useCreateWebhook(workspaceSlug, (created) => {
    if (writeSession.current === null || writeSession.current !== session.current) return;
    writeSession.current = null;
    notifySuccess(WEBHOOK_CREATED_NOTICE);
    downloadWebhookBackup(webhookBackup(created, workspaceSlug, created.secret_key, Date.now()));
    setIssued({ webhook: created, secret: created.secret_key });
    onCreated?.(created);
  });
  const [url, setUrl] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);
  const [selection, setSelection] = React.useState<WebhookEventSelection>(initialEventSelection);
  const reveal = issued !== null;

  React.useEffect(() => {
    if (open) return;
    // Closing forgets everything: typed address, errors, the key and the
    // session a write was sent in, so no later render can bring the key
    // back and no in-flight answer can surface one (§20).
    session.current += 1;
    writeSession.current = null;
    setUrl("");
    setError(null);
    setSelection(initialEventSelection());
    setIssued(null);
  }, [open]);

  const handleOpenChange = (next: boolean, details: { reason: string }): void => {
    const target = nextWorkspaceSettingsDialogOpen(open, { open: next, reason: details.reason }, { reveal });
    if (target !== open) onOpenChange(target);
  };

  const submit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    const address = normalizeWebhookUrl(url);
    if (address === "") {
      setError(WEBHOOK_URL_REQUIRED);
      return;
    }
    setError(null);
    const flags = submittedFlags(selection);
    const submittedIn = session.current;
    writeSession.current = submittedIn;
    void create.mutateAsync({ url: address, ...flags }).catch(() => {
      if (writeSession.current === submittedIn && session.current === submittedIn) {
        writeSession.current = null;
        notifyError(WEBHOOK_NOT_CREATED_NOTICE, WEBHOOK_NOT_CREATED_BODY);
      }
    });
  };

  if (reveal && issued !== null) {
    return (
      <Dialog
        open={open}
        onOpenChange={handleOpenChange}
        title={KEY_CREATED_TITLE}
        nonDismissable
        actions={
          <Button variant="primary" onClick={() => onOpenChange(false)}>
            Close
          </Button>
        }
      >
        <div className="flex flex-col gap-(--space-4)">
          <p className="text-body text-(--text-muted)">{KEY_CREATED_EXPLAINER}</p>
          <SecretKey secret={issued.secret} />
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog
      open={open}
      onOpenChange={handleOpenChange}
      title={CREATE_WEBHOOK_TITLE}
      actions={
        <>
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" form="webhook-create-form" loading={create.isPending}>
            Create
          </Button>
        </>
      }
    >
      <form id="webhook-create-form" className="flex flex-col gap-(--space-6)" onSubmit={submit} noValidate>
        <Input
          label="Target URL"
          placeholder="https://example.com/post"
          value={url}
          autoFocus
          required
          onChange={(event) => {
            setUrl(event.target.value);
            if (error !== null) setError(null);
          }}
          {...(error === null ? {} : { error })}
        />
        <EventPicker value={selection} onChange={setSelection} />
      </form>
    </Dialog>
  );
}
