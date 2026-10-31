// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The context empty states of the issues area (parity/specs/issues.md §8,
// ISS-068–074). Each card states what the context holds, why it is empty
// and, where the spec allows one, the entry that fills it: creation, an
// add-existing search, clearing the filters, or the project's automation
// settings. Profile tabs are informational by design, so they never carry
// an action.

import * as React from "react";

import { ArchiveArt, FirstItemArt, NoMatchArt, ProfileArt, ProjectArt, ViewArt } from "./artwork.js";
import { EmptyContextCard } from "./EmptyContextCard.js";

/** Heading the filtered-to-zero card shares across every list. */
export const NO_MATCH_TITLE = "No matching results.";
export const NO_MATCH_DESCRIPTION = "No results found. Try adjusting your search terms.";

export interface FilteredEmptyCardProps {
  /** Context the card belongs to, e.g. "cycle". */
  context: string;
  /** The list names the entry its own way: "Clear all filters" or "Clear filters". */
  clearLabel: string;
  /** Guest standing leaves the entry visible and disabled. */
  canAct: boolean;
  onClear: () => void;
}

/** The card a list shows when its stored expression matches no rows. */
export function FilteredEmptyCard({
  context,
  clearLabel,
  canAct,
  onClear,
}: FilteredEmptyCardProps): React.ReactElement {
  return (
    <EmptyContextCard
      context={`${context}-filtered`}
      title={NO_MATCH_TITLE}
      description={NO_MATCH_DESCRIPTION}
      artwork={<NoMatchArt />}
      actions={[{ label: clearLabel, onSelect: onClear, disabled: !canAct }]}
    />
  );
}

export interface ProjectIssuesEmptyProps {
  canAct: boolean;
  onCreate: () => void;
}

/** ISS-068: a project that holds no work items at all. */
export function ProjectIssuesEmpty({ canAct, onCreate }: ProjectIssuesEmptyProps): React.ReactElement {
  return (
    <EmptyContextCard
      context="project"
      title="Start with your first work item."
      description="Work items are the building blocks of your project — assign owners, set priorities, and track progress easily."
      artwork={<FirstItemArt />}
      actions={[{ label: "Create your first work item", onSelect: onCreate, disabled: !canAct }]}
    />
  );
}

export interface CycleIssuesEmptyProps {
  /** A cycle whose dates are behind it reads as a record, not a workspace. */
  completed: boolean;
  canAct: boolean;
  onCreate: () => void;
  onAddExisting: () => void;
}

/** ISS-069: a cycle with nothing on its list. */
export function CycleIssuesEmpty({
  completed,
  canAct,
  onCreate,
  onAddExisting,
}: CycleIssuesEmptyProps): React.ReactElement {
  if (completed) {
    return (
      <EmptyContextCard
        context="cycle-completed"
        title="No work items in the cycle"
        description="Work items are either transferred or hidden. To see hidden work items, if any, update your display options."
        artwork={<FirstItemArt />}
      />
    );
  }
  return (
    <EmptyContextCard
      context="cycle"
      title="No work items to show in this cycle"
      description="Create work items to begin monitoring your team's progress this cycle and achieve your goals on time."
      artwork={<FirstItemArt />}
      actions={[
        { label: "Create work item", onSelect: onCreate, disabled: !canAct },
        { label: "Add existing work item", onSelect: onAddExisting, disabled: !canAct },
      ]}
    />
  );
}

export interface ModuleIssuesEmptyProps {
  canAct: boolean;
  onCreate: () => void;
  onAddExisting: () => void;
}

/** ISS-070: a module with nothing on its list. */
export function ModuleIssuesEmpty({ canAct, onCreate, onAddExisting }: ModuleIssuesEmptyProps): React.ReactElement {
  return (
    <EmptyContextCard
      context="module"
      title="No work items to show in this Module"
      description="Create work items to begin monitoring this module."
      artwork={<FirstItemArt />}
      actions={[
        { label: "Create work item", onSelect: onCreate, disabled: !canAct },
        { label: "Add existing work item", onSelect: onAddExisting, disabled: !canAct },
      ]}
    />
  );
}

export interface ArchivedIssuesEmptyProps {
  canAct: boolean;
  onSetAutomation: () => void;
}

/** ISS-071: the archive holds nothing; the entry here is the automation rule. */
export function ArchivedIssuesEmpty({ canAct, onSetAutomation }: ArchivedIssuesEmptyProps): React.ReactElement {
  return (
    <EmptyContextCard
      context="archived"
      title="No archived work items yet"
      description="Manually or through automation, you can archive work items that are completed or cancelled. Find them here once archived."
      artwork={<ArchiveArt />}
      actions={[{ label: "Set automation", onSelect: onSetAutomation, disabled: !canAct }]}
    />
  );
}

export interface SavedViewIssuesEmptyProps {
  canAct: boolean;
  onCreate: () => void;
}

/** ISS-072: a saved view whose conditions match no work item yet. */
export function SavedViewIssuesEmpty({ canAct, onCreate }: SavedViewIssuesEmptyProps): React.ReactElement {
  return (
    <EmptyContextCard
      context="view"
      title="View work items will appear here"
      description="Work items help you track individual pieces of work. With work items, keep track of what's going on, who is working on it, and what's done."
      artwork={<ViewArt />}
      actions={[{ label: "New work item", onSelect: onCreate, disabled: !canAct }]}
    />
  );
}

export interface GlobalIssuesEmptyProps {
  /** Projects the caller can see; zero picks the project card instead. */
  projectCount: number;
  canAct: boolean;
  onCreateProject: () => void;
  onCreateWorkItem: () => void;
}

/** ISS-073: the workspace all-issues surface before it can list anything. */
export function GlobalIssuesEmpty({
  projectCount,
  canAct,
  onCreateProject,
  onCreateWorkItem,
}: GlobalIssuesEmptyProps): React.ReactElement {
  if (projectCount === 0) {
    return (
      <EmptyContextCard
        context="global-no-project"
        title="No project"
        description="To create work items or manage your work, you need to create a project or be a part of one."
        artwork={<ProjectArt />}
        actions={[{ label: "Start your first project", onSelect: onCreateProject, disabled: !canAct }]}
      />
    );
  }
  return (
    <EmptyContextCard
      context="global"
      title="No Views yet"
      description="Add work items to your project and use views to filter, sort, and monitor progress effortlessly."
      artwork={<ViewArt />}
      actions={[{ label: "Add work item", onSelect: onCreateWorkItem, disabled: !canAct }]}
    />
  );
}

/** Profile tabs that list work items by ownership (ISS-074). */
export type ProfileIssueTab = "assigned" | "created" | "subscribed";

const PROFILE_COPY: Record<ProfileIssueTab, { title: string; description: string }> = {
  assigned: {
    title: "No work items are assigned to you",
    description: "Work items assigned to you can be tracked from here.",
  },
  created: {
    title: "No work items yet",
    description: "All work items created by you come here, track them here directly.",
  },
  subscribed: {
    title: "No work items yet",
    description: "Subscribe to work items you are interested in, track all of them here.",
  },
};

export interface ProfileIssuesEmptyProps {
  tab: ProfileIssueTab;
}

/** ISS-074: an informational card for a profile tab holding no rows. */
export function ProfileIssuesEmpty({ tab }: ProfileIssuesEmptyProps): React.ReactElement {
  const copy = PROFILE_COPY[tab];
  return (
    <EmptyContextCard
      context={`profile-${tab}`}
      title={copy.title}
      description={copy.description}
      artwork={<ProfileArt />}
    />
  );
}
