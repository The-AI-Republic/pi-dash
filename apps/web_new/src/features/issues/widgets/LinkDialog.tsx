// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add/edit external link (parity/specs/issues.md §15, ISS-185). One form
// serves both: an empty draft creates, a draft with an id patches. The
// URL is required; a scheme-less address gets http:// before it is sent,
// and any other string is left for the server to judge.

import * as React from "react";

import { Button, Dialog, Input } from "@pidash/kit";
import type { IssueLink } from "@pidash/api-client";

import { normalizeLinkUrl } from "./api.js";

export const LINK_URL_REQUIRED = "Enter a URL.";

export interface LinkDraft {
  url: string;
  title: string;
}

export interface LinkDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Existing row when editing; null when adding. */
  link: IssueLink | null;
  pending: boolean;
  onSubmit: (draft: LinkDraft) => void;
}

export function LinkDialog({ open, onOpenChange, link, pending, onSubmit }: LinkDialogProps): React.ReactElement {
  const [url, setUrl] = React.useState("");
  const [title, setTitle] = React.useState("");
  const [invalid, setInvalid] = React.useState(false);

  React.useEffect(() => {
    if (!open) return;
    setUrl(link?.url ?? "");
    setTitle(link?.title ?? "");
    setInvalid(false);
  }, [open, link]);

  function submit(): void {
    if (url.trim() === "") {
      setInvalid(true);
      return;
    }
    onSubmit({ url: normalizeLinkUrl(url), title: title.trim() });
  }

  return (
    <Dialog
      title={link === null ? "Add link" : "Edit link"}
      description={link === null ? "Point this work item at an external page." : undefined}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={pending} onClick={submit}>
            {link === null ? "Add link" : "Save link"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <div className="flex flex-col gap-(--space-1)">
          <Input
            label="URL"
            autoFocus
            aria-invalid={invalid}
            placeholder="Type or paste a URL"
            value={url}
            onChange={(event) => {
              setUrl(event.target.value);
              setInvalid(false);
            }}
          />
          {invalid ? (
            <span role="alert" className="text-meta text-(--danger)">
              {LINK_URL_REQUIRED}
            </span>
          ) : null}
        </div>
        <div className="flex flex-col gap-(--space-1)">
          <Input
            label="Display title (optional)"
            placeholder="What this link should read as"
            value={title}
            onChange={(event) => {
              setTitle(event.target.value);
            }}
          />
        </div>
      </div>
    </Dialog>
  );
}
