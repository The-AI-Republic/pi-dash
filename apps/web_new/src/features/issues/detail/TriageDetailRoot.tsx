// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Read-only triage detail (parity/specs/notifications.md §5 and §12,
// NTF-008). The issues area owns triage content; the notification detail
// pane only embeds it. The decision strip mirrors what a triage item
// offers and stays inert here: accepting or declining is the Intake
// area's action, and this surface just shows the request. Until intake
// ships its decision flow this host renders the request read-only —
// the explicit stub seam §12 allows — behind the same props the real
// content will take.
import type { Issue } from "@pidash/api-client";
import { Check, X } from "lucide-react";
import * as React from "react";

import { Button } from "@pidash/kit";
import { IssueDetailRoot } from "./IssueDetailRoot.js";

export interface TriageDetailRootProps {
  workspaceSlug: string;
  projectId: string;
  issue: Issue;
  reference: string;
}

export function TriageDetailRoot({
  workspaceSlug,
  projectId,
  issue,
  reference,
}: TriageDetailRootProps): React.ReactElement {
  return (
    <div data-testid="issue-triage-root" className="flex min-h-0 flex-col">
      <section
        aria-label="Triage decision"
        className="flex flex-wrap items-center justify-between gap-(--space-3) border-b border-(--border) px-(--space-5) py-(--space-3)"
      >
        <p className="text-meta text-(--text-muted)">Waiting for a triage decision on {reference}.</p>
        <div className="flex items-center gap-(--space-2)">
          <Button size="small" disabled title="Triage decisions are made from the Intake screen">
            <Check size={14} strokeWidth={1.5} aria-hidden="true" />
            Accept
          </Button>
          <Button size="small" variant="danger" disabled title="Triage decisions are made from the Intake screen">
            <X size={14} strokeWidth={1.5} aria-hidden="true" />
            Decline
          </Button>
        </div>
      </section>
      <IssueDetailRoot
        workspaceSlug={workspaceSlug}
        projectId={projectId}
        issue={issue}
        editable={false}
        topSlot={<span className="text-meta text-(--text-muted)">{reference}</span>}
      />
    </div>
  );
}
