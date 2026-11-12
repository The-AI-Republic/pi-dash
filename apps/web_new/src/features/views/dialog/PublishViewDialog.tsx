// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Publish dialog for a saved project view (parity/specs/views.md §6,
// VIEW-020, cloud only). The dialog only shows: the public read-only
// link with a copy control while published, the publish offer while
// not. The caller owns the slot writes, the cache adoption and the
// notices. The OSS edition never mounts this (no publish slot, and its
// server never sets an anchor).

import { Button, Dialog } from "@pidash/kit";
import * as React from "react";

export interface PublishViewDialogProps {
  open: boolean;
  viewName: string;
  /** Public read-only address, or null while the view is unpublished. */
  link: string | null;
  /** True while a publish or unpublish write is in flight. */
  pending?: boolean;
  onPublish: () => void;
  onUnpublish: () => void;
  /** Copies the displayed link; called with the link text. */
  onCopyLink: (link: string) => void;
  onClose: () => void;
}

export function PublishViewDialog({
  open,
  viewName,
  link,
  pending = false,
  onPublish,
  onUnpublish,
  onCopyLink,
  onClose,
}: PublishViewDialogProps): React.ReactElement {
  const published = link !== null;
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={published ? "Published view" : "Publish view"}
      description={
        published
          ? `Anyone with this link can read ${viewName}. Unpublishing switches the link off.`
          : `Publishing ${viewName} creates a public read-only link anyone can open.`
      }
      actions={
        <>
          <Button variant="secondary" onClick={onClose} disabled={pending}>
            Cancel
          </Button>
          {published ? (
            <Button variant="danger" onClick={onUnpublish} disabled={pending}>
              {pending ? "Unpublishing…" : "Unpublish"}
            </Button>
          ) : (
            <Button variant="primary" onClick={onPublish} disabled={pending}>
              {pending ? "Publishing…" : "Publish"}
            </Button>
          )}
        </>
      }
    >
      {link === null ? (
        <p className="text-body text-(--text-muted)">The row shows a Live marker while the link works.</p>
      ) : (
        <div className="flex items-center gap-(--space-3)">
          <p data-testid="publish-view-link" className="text-body min-w-0 flex-1 truncate text-(--text)">
            {link}
          </p>
          <Button
            variant="secondary"
            onClick={() => {
              onCopyLink(link);
            }}
            disabled={pending}
          >
            Copy link
          </Button>
        </div>
      )}
    </Dialog>
  );
}
