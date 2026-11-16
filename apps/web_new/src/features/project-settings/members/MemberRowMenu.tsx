// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Row hover menu and its confirmation (parity/specs/project-settings-members.md
// §"Remove and leave-self"; PSET-024/025). The name cell reveals a menu on
// hover and focus: "Remove" for admins acting on someone else, "Leave" on
// the caller's own row. Both open the same confirmation; the remove copy
// names the member and warns that access is gone for good, the leave copy
// asks to leave the project.

import { Button, Dialog, IconButton, Menu, type MenuEntry } from "@pidash/kit";
import { MoreHorizontal } from "lucide-react";
import * as React from "react";

import type { MemberRowAction } from "./writes.js";

export const REMOVE_CONFIRM_VERB = "Remove";
export const LEAVE_CONFIRM_VERB = "Leave";

/** Confirmation copy; the remove sentence names the member asked about. */
export function confirmationCopy(
  action: Exclude<MemberRowAction, "none">,
  name: string
): {
  title: string;
  description: string;
  confirm: string;
} {
  if (action === "leave") {
    return {
      title: "Leave project?",
      description:
        "If you leave this project you will lose access to its work items and settings. " +
        "You can rejoin if you are invited again.",
      confirm: LEAVE_CONFIRM_VERB,
    };
  }
  return {
    title: `Remove ${name}?`,
    description: `${name} will no longer have access to this project. Their access is removed permanently.`,
    confirm: REMOVE_CONFIRM_VERB,
  };
}

export interface MemberRowMenuProps {
  action: MemberRowAction;
  /** Display name of the row owner; used by the removal copy. */
  name: string;
  pending: boolean;
  onConfirm: () => void;
  /** Held open while the menu itself is open (hover and keyboard alike). */
  className?: string;
}

/**
 * The menu plus its confirmation. Opening the confirmation closes the menu
 * so the dialog is the only surface in front of the table; a cancel leaves
 * the membership untouched and a confirm runs the caller's write.
 */
export function MemberRowMenu({
  action,
  name,
  pending,
  onConfirm,
  className,
}: MemberRowMenuProps): React.ReactElement | null {
  const [open, setOpen] = React.useState(false);
  const [confirming, setConfirming] = React.useState<Exclude<MemberRowAction, "none"> | null>(null);
  // The dialog withdraws once the write settles, whatever its outcome: a
  // refusal is reported by the toast, not by leaving the confirmation up.
  const submitted = React.useRef(false);
  React.useEffect(() => {
    if (submitted.current && !pending) {
      submitted.current = false;
      setConfirming(null);
    }
  }, [pending]);

  if (action === "none") return null;
  const verb = action === "leave" ? LEAVE_CONFIRM_VERB : REMOVE_CONFIRM_VERB;
  const items: MenuEntry[] = [
    {
      label: verb,
      onSelect: () => {
        setOpen(false);
        setConfirming(action);
      },
    },
  ];
  const copy = confirming === null ? null : confirmationCopy(confirming, name);

  return (
    <>
      <Menu
        open={open}
        onOpenChange={setOpen}
        trigger={
          <IconButton
            label={`Actions for ${name}`}
            size="small"
            className={className}
            data-testid="member-row-menu-trigger"
          >
            <MoreHorizontal size={14} strokeWidth={1.5} />
          </IconButton>
        }
        items={items}
      />
      {copy === null ? null : (
        <Dialog
          open
          title={copy.title}
          description={copy.description}
          onOpenChange={(next) => {
            if (!next) setConfirming(null);
          }}
          nonDismissable={pending}
          actions={
            <>
              <Button variant="secondary" disabled={pending} onClick={() => setConfirming(null)}>
                Cancel
              </Button>
              <Button
                variant="danger"
                loading={pending}
                onClick={() => {
                  submitted.current = true;
                  onConfirm();
                }}
              >
                {copy.confirm}
              </Button>
            </>
          }
        >
          <span className="hidden" aria-hidden="true" />
        </Dialog>
      )}
    </>
  );
}
