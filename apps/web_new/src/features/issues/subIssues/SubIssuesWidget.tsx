// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Detail widgets root (parity/specs/issues.md §15, ISS-173). The action
// row always offers the five entries — add sub-work item, add relation,
// add link, attach, manually run AI — and the collapsibles follow in a
// fixed order: sub-work items, relations, links, attachments. A section
// renders only when it has content: the sub-issues section reads the
// parent's children and hides itself when there are none, and the later
// widgets arrive through their own slices' slots. Guests read the same
// action row with every write entry disabled.

import { Button, Menu, type MenuEntry } from "@pidash/kit";
import type { Issue, ProjectDetails, SubIssueItem } from "@pidash/api-client";
import { Link2, Paperclip, Plus, Split, Sparkles, Workflow } from "lucide-react";
import * as React from "react";

import type { WorkItemCreateProject } from "../components/WorkItemCreateDialog.js";
import { SubIssueSection, type SubIssueSectionHandle } from "./SubIssueSection.js";
import { subIssueTitle } from "./rules.js";

/** The five action-row entries, in render order. */
export const WIDGET_ACTION_LABELS = [
  "Add sub-work item",
  "Add relation",
  "Add link",
  "Attach",
  "Manually Run AI",
] as const;

export type WidgetActionLabel = (typeof WIDGET_ACTION_LABELS)[number];

/** Entries this slice implements; the rest wait for their owning slices. */
export const SUB_ISSUES_WIDGET_ACTIONS: readonly WidgetActionLabel[] = ["Add sub-work item"];

function actionIcon(label: WidgetActionLabel): React.ReactElement {
  const props = { size: 14, strokeWidth: 1.5, "aria-hidden": "true" as const };
  switch (label) {
    case "Add sub-work item":
      return <Workflow {...props} />;
    case "Add relation":
      return <Split {...props} />;
    case "Add link":
      return <Link2 {...props} />;
    case "Attach":
      return <Paperclip {...props} />;
    case "Manually Run AI":
      return <Sparkles {...props} />;
  }
}

/** A later widget's collapsible slot, in the fixed order. */
export interface WidgetSectionSlot {
  label: "Relations" | "Links" | "Attachments";
  node: React.ReactNode;
}

export interface SubIssuesWidgetProps {
  workspaceSlug: string;
  issue: Issue;
  project: ProjectDetails | undefined;
  editable: boolean;
  projectIdentifier: string;
  canWrite: boolean;
  /** Opens a child at the browse key built from its own project code. */
  onOpen: (row: SubIssueItem, identifier: string) => void;
  createProjects: WorkItemCreateProject[];
  /** Sections relations/links/attachments slices mount here. */
  extraSections?: readonly WidgetSectionSlot[] | undefined;
}

export function SubIssuesWidget({
  workspaceSlug,
  issue,
  project,
  editable,
  projectIdentifier,
  canWrite,
  onOpen,
  createProjects,
  extraSections = [],
}: SubIssuesWidgetProps): React.ReactElement {
  const section = React.useRef<SubIssueSectionHandle>(null);
  const [addMenuOpen, setAddMenuOpen] = React.useState(false);
  const canAdd = canWrite && editable;
  const title = subIssueTitle(issue.is_epic ?? false);

  const addEntries: MenuEntry[] = [
    {
      label: "Create new",
      onSelect: () => section.current?.openCreate(),
      icon: <Plus size={14} strokeWidth={1.5} aria-hidden="true" />,
    },
    {
      label: "Add existing",
      onSelect: () => section.current?.openExisting(),
      icon: <Workflow size={14} strokeWidth={1.5} aria-hidden="true" />,
    },
  ];

  return (
    <section
      aria-label="Work item widgets"
      className="flex flex-col gap-(--space-4)"
      data-testid="issue-detail-widgets"
    >
      <div className="flex flex-wrap items-center gap-(--space-2)">
        {WIDGET_ACTION_LABELS.map((label) => {
          const implemented = SUB_ISSUES_WIDGET_ACTIONS.includes(label);
          const button = (
            <Button variant="secondary" size="medium" disabled={!canAdd || !implemented} data-widget-action={label}>
              <span className="flex items-center gap-(--space-1)">
                {actionIcon(label)}
                <span>{label}</span>
              </span>
            </Button>
          );
          if (label !== "Add sub-work item") {
            return <React.Fragment key={label}>{button}</React.Fragment>;
          }
          return (
            <Menu key={label} open={addMenuOpen} onOpenChange={setAddMenuOpen} trigger={button} items={addEntries} />
          );
        })}
      </div>
      <div className="flex min-w-0 flex-col divide-y divide-(--border)">
        <SubIssueSection
          ref={section}
          workspaceSlug={workspaceSlug}
          issue={issue}
          project={project}
          editable={editable}
          projectIdentifier={projectIdentifier}
          canWrite={canAdd}
          onOpen={onOpen}
          createProjects={createProjects}
          heading={title}
        />
        {extraSections.map((slot) => (
          <div key={slot.label} data-widget-section={slot.label}>
            {slot.node}
          </div>
        ))}
      </div>
    </section>
  );
}
