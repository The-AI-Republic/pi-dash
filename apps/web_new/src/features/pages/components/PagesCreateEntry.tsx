// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page creation entry points (parity/specs/pages.md §3.6, PAGE-007/008).
// Two ways to a new page with one place to open it from: the header's
// primary action posts straight through with no form and lands on the new
// page, and the palette's "New page" command opens the form dialog. Both
// appear only for members and admins; a failed quick create only reports
// and leaves the operator on the list.

import { Button } from "@pidash/kit";
import { FilePlus2 } from "lucide-react";
import * as React from "react";

import { registerCommands } from "../../../shared/commands/index.js";
import { quickCreateAccess } from "../lib/pageCreate.js";
import type { PageListTab } from "../lib/pageList.js";
import { PageCreateDialog } from "./PageCreateDialog.js";

/** Stable palette command id for the dialog path. */
export const CREATE_PAGE_COMMAND_ID = "pages.create-page";
/** The command title and two-key sequence the oracle uses to reach the form. */
export const CREATE_PAGE_COMMAND_TITLE = "New page";
export const CREATE_PAGE_COMMAND_SEQUENCE = "nd";

export interface PagesQuickCreateProps {
  /** True while the create request flies; the button shows its spinner. */
  pending: boolean;
  onPress: () => void;
}

/**
 * Header primary action face: one press, no form. The list owns the write
 * so the header button and the zero-state action share one mutation.
 */
export function PagesQuickCreate({ pending, onPress }: PagesQuickCreateProps): React.ReactElement {
  return (
    <Button variant="primary" size="medium" loading={pending} data-testid="pages-quick-create" onClick={onPress}>
      <FilePlus2 size={14} strokeWidth={1.5} aria-hidden="true" />
      Add page
    </Button>
  );
}

export interface PageCreateDialogEntryProps {
  workspaceSlug: string;
  projectId: string;
  tab: PageListTab;
  /** Runs with the new page's id after the dialog closes. */
  onOpenPage: (pageId: string) => void;
  /** False for guests, who get neither the command nor the dialog. */
  allowed: boolean;
}

/**
 * Registers the palette command and mounts the dialog it opens. The
 * command keeps the old palette's title and two-key sequence so muscle
 * memory and the oracle both reach the form; guests never see it.
 */
export function PageCreateDialogEntry({
  workspaceSlug,
  projectId,
  tab,
  onOpenPage,
  allowed,
}: PageCreateDialogEntryProps): React.ReactElement | null {
  const [open, setOpen] = React.useState(false);
  const [session, setSession] = React.useState(0);

  React.useEffect(() => {
    if (!allowed) return undefined;
    return registerCommands([
      {
        id: CREATE_PAGE_COMMAND_ID,
        title: CREATE_PAGE_COMMAND_TITLE,
        section: "Create",
        sequence: CREATE_PAGE_COMMAND_SEQUENCE,
        keywords: ["create", "add", "new", "page", "document"],
        run: () => {
          setSession((count) => count + 1);
          setOpen(true);
        },
      },
    ]);
  }, [allowed]);

  if (!allowed || !open) return null;
  return (
    <PageCreateDialog
      key={session}
      open={open}
      onOpenChange={setOpen}
      workspaceSlug={workspaceSlug}
      projectId={projectId}
      initialAccess={quickCreateAccess(tab)}
      onCreated={onOpenPage}
    />
  );
}
