// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Token rows and deletion (parity/specs/workspace-settings.md §28,
// WSET-028): each row names the token, marks it Active or Expired from the
// server's liveness flag, shows its description and human expiry line, and
// exposes a hover/focus delete control. Deletion asks for confirmation
// naming the irreversible loss of access.

import type { PersonalApiToken } from "@pidash/api-client";
import { Badge, Button, Dialog, IconButton } from "@pidash/kit";
import { Trash2 } from "lucide-react";
import * as React from "react";

import { useDeletePersonalToken } from "./api.js";
import { isExpiredToken, rowDescription, rowExpiryLine, TOKEN_DELETE_TITLE } from "./model.js";

export interface TokenListProps {
  tokens: readonly PersonalApiToken[];
  now?: Date | undefined;
  /** Member timezone for the expiry lines (UTC until the member picks one). */
  timeZone?: string | undefined;
}

export function TokenList({ tokens, now = new Date(), timeZone = "UTC" }: TokenListProps): React.ReactElement {
  const [pendingDelete, setPendingDelete] = React.useState<PersonalApiToken | null>(null);
  const remove = useDeletePersonalToken();

  return (
    <>
      <ul className="divide-y divide-(--border) rounded-(--radius-control) border border-(--border)">
        {tokens.map((token) => (
          <li
            key={token.id}
            data-token-id={token.id}
            className="group grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-(--space-4) gap-y-(--space-1) px-(--space-4) py-(--space-3)"
          >
            <div className="flex items-center gap-(--space-2)">
              <h5 className="text-emphasis truncate font-medium text-(--text)">{token.label}</h5>
              <Badge tone={isExpiredToken(token) ? "warning" : "success"} dot>
                {isExpiredToken(token) ? "Expired" : "Active"}
              </Badge>
            </div>
            <IconButton
              label={`Delete ${token.label}`}
              size="small"
              variant="danger"
              className="row-span-3 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
              onClick={() => setPendingDelete(token)}
            >
              <Trash2 size={14} strokeWidth={1.5} aria-hidden="true" />
            </IconButton>
            {rowDescription(token) === null ? null : (
              <p className="text-meta col-start-1 text-(--text-muted)">{rowDescription(token)}</p>
            )}
            <p className="text-meta col-start-1 text-(--text-muted)">{rowExpiryLine(token, now, timeZone)}</p>
          </li>
        ))}
      </ul>
      <Dialog
        title={TOKEN_DELETE_TITLE}
        description={
          pendingDelete === null
            ? ""
            : `Deleting ${pendingDelete.label} revokes its access for any application or script using it. This cannot be undone.`
        }
        open={pendingDelete !== null}
        onOpenChange={(next) => {
          if (!next) setPendingDelete(null);
        }}
        nonDismissable={remove.isPending}
        actions={
          pendingDelete === null ? null : (
            <>
              <Button variant="secondary" onClick={() => setPendingDelete(null)} disabled={remove.isPending}>
                Cancel
              </Button>
              <Button
                variant="danger"
                loading={remove.isPending}
                onClick={() =>
                  remove.mutate(pendingDelete.id, {
                    onSuccess: () => setPendingDelete(null),
                    onError: () => setPendingDelete(null),
                  })
                }
              >
                Delete
              </Button>
            </>
          )
        }
      >
        {null}
      </Dialog>
    </>
  );
}
