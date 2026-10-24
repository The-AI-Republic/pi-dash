// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add/edit module link dialog (parity/specs/modules.md §9.3, MOD-035).
// One form serves both: a null link creates, a row patches. The URL is
// required and invalid-marked when empty (no separate error sentence);
// a scheme-less address gets http:// before it is sent, and the title
// stays optional. Cancel, Escape and the backdrop discard the draft.

import * as React from "react";

import { Button, Dialog, Input } from "@pidash/kit";

import {
  MODULE_LINK_ADD_TITLE,
  MODULE_LINK_UPDATE_TITLE,
  defaultModuleLinkScheme,
  type ModuleLinkSummary,
} from "./moduleLinksModel.js";

export interface ModuleLinkDraft {
  url: string;
  title: string | null;
}

export interface ModuleLinkDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Existing row when editing; null when adding. */
  link: ModuleLinkSummary | null;
  pending: boolean;
  onSubmit: (draft: ModuleLinkDraft) => void;
}

export function ModuleLinkDialog({
  open,
  onOpenChange,
  link,
  pending,
  onSubmit,
}: ModuleLinkDialogProps): React.ReactElement {
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
    const trimmedTitle = title.trim();
    onSubmit({ url: defaultModuleLinkScheme(url), title: trimmedTitle === "" ? null : trimmedTitle });
  }

  return (
    <Dialog
      title={link === null ? MODULE_LINK_ADD_TITLE : MODULE_LINK_UPDATE_TITLE}
      open={open}
      onOpenChange={onOpenChange}
      actions={
        <>
          <Button variant="secondary" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="primary" loading={pending} data-testid="modules-link-submit" onClick={submit}>
            {link === null ? MODULE_LINK_ADD_TITLE : MODULE_LINK_UPDATE_TITLE}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-(--space-4)">
        <Input
          label="URL"
          autoFocus
          aria-invalid={invalid}
          className={invalid ? "border-(--danger) focus:border-(--danger)" : undefined}
          placeholder="Paste the link address"
          value={url}
          onChange={(event) => {
            setUrl(event.target.value);
            setInvalid(false);
          }}
        />
        <Input
          label="Display title"
          hint="Optional"
          placeholder="Name this link"
          value={title}
          onChange={(event) => {
            setTitle(event.target.value);
          }}
        />
      </div>
    </Dialog>
  );
}
