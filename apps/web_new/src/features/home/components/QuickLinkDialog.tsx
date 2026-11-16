// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Saved-link add/edit dialog (SHELL-012; written from parity/specs/shell.md
// §2). The address is required and must be a web link; a missing scheme is
// completed on save. Submit validates inline, shows progress while the
// write lands, and keeps the dialog open with a failure notice when the
// write fails; cancel always discards without storing.

import * as React from "react";

import type { QuickLink } from "@pidash/api-client";
import { Button, Dialog, Input } from "@pidash/kit";

import { useCreateQuickLink, useUpdateQuickLink } from "../api/quicklinks.js";
import { validateQuickLinkForm } from "../lib/quicklinks.js";

export interface QuickLinkDialogProps {
  workspaceSlug: string;
  /** The link under edit, or null for a fresh link. */
  link: QuickLink | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function QuickLinkDialog({ workspaceSlug, link, open, onOpenChange }: QuickLinkDialogProps): React.ReactElement {
  const [address, setAddress] = React.useState(link?.url ?? "");
  const [name, setName] = React.useState(link?.title ?? "");
  const [error, setError] = React.useState<string | null>(null);
  const create = useCreateQuickLink(workspaceSlug);
  const update = useUpdateQuickLink(workspaceSlug);
  const pending = create.isPending || update.isPending;

  function submit(): void {
    if (pending) return;
    const validation = validateQuickLinkForm({ address, name });
    if (!validation.ok) {
      setError(validation.error);
      return;
    }
    setError(null);
    const close = (): void => {
      onOpenChange(false);
    };
    if (link === null) {
      create.mutate(validation.value, { onSuccess: close });
    } else {
      update.mutate({ ...validation.value, id: link.id }, { onSuccess: close });
    }
  }

  return (
    <Dialog
      title={link === null ? "Add link" : "Edit link"}
      open={open}
      onOpenChange={onOpenChange}
      nonDismissable={pending}
      actions={
        <>
          <Button variant="secondary" disabled={pending} onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={pending} onClick={submit}>
            Save link
          </Button>
        </>
      }
    >
      <form
        aria-label={link === null ? "Add link" : "Edit link"}
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        className="flex flex-col gap-(--space-6)"
      >
        <Input
          label="Address"
          value={address}
          onChange={(event) => {
            setAddress(event.target.value);
            setError(null);
          }}
          {...(error === null ? {} : { error })}
          placeholder="example.com/docs"
          autoComplete="url"
          disabled={pending}
        />
        <Input
          label="Name"
          hint="Optional display name"
          value={name}
          onChange={(event) => {
            setName(event.target.value);
          }}
          placeholder="Team docs"
          autoComplete="off"
          disabled={pending}
        />
      </form>
    </Dialog>
  );
}
