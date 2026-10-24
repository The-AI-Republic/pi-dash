// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cloud data seams for the edition-only issues surfaces (§20/§23). The
// flags decide whether a surface may render; these providers decide what
// it knows. A flag turned on without its provider still renders nothing,
// so a misbuilt community bundle stays as empty as the oracle asserts.
// The cloud module registers them once at bootstrap, exactly as the
// issue-type seam works; the community build never calls register.

import type { State } from "@pidash/api-client";
import type { ComponentType } from "react";

/** Where a transition is being attempted, so rules can differ by surface. */
export type TransitionPurpose = "state-picker" | "board-drop" | "create";

/** One refused transition: the sentence the surface shows. */
export interface WorkflowRefusal {
  reason: string;
}

/** The transition rules a cloud workflow configuration enforces. */
export interface WorkflowRules {
  /**
   * Refusal for moving an item from `fromStateId` (null for a new item)
   * into the target state, or null when the move is allowed.
   */
  refusal: (input: {
    projectId: string;
    fromStateId: string | null;
    toStateId: string;
    purpose: TransitionPurpose;
  }) => WorkflowRefusal | null;
  /** Refusal for creating an item inside the given board column, or null. */
  creationRefusal?: (input: { projectId: string; columnKey: string }) => WorkflowRefusal | null;
}

/** A team-project row the scoping filter offers. */
export interface TeamProjectOption {
  id: string;
  name: string;
  identifier: string;
}

/** One work-item type the type picker offers. */
export interface WorkItemTypeOption {
  id: string;
  name: string;
  color?: string | null;
}

/** Properties the detail badge surface reads for one issue. */
export interface DetailBadgeInput {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  cycleId: string | null;
}

/** Accents the cloud detail sidebar adds beside the due date and cycle. */
export interface DetailBadgeSlot {
  /** Note shown under the cycle row for an item moved between cycles. */
  transferNote?: ComponentType<DetailBadgeInput>;
}

/** Editable epic payload the cloud dialog submits (ISS-232). */
export interface EpicDraftValues {
  name: string;
  descriptionHtml: string;
  stateId: string | null;
  priority: string;
  assigneeIds: string[];
  labelIds: string[];
}

/** Detail-side type surface: badge/switcher, properties and activity. */
export interface IssueTypeDetailSlot {
  /** Type badge or switcher beside the identifier. */
  badge?: ComponentType<{ workspaceSlug: string; projectId: string; issueId: string; editable: boolean }>;
  /** Additional custom properties under the sidebar. */
  properties?: ComponentType<{ workspaceSlug: string; projectId: string; issueId: string; editable: boolean }>;
  /** Type and property activity rows in the feed. */
  activity?: ComponentType<{ workspaceSlug: string; projectId: string; issueId: string }>;
}

/** The fields this build may fill; any may stay absent. */
export interface EditionIssuesProviders {
  workflowRules?: WorkflowRules;
  teamProjects?: (input: { workspaceSlug: string; projectId: string }) => Promise<TeamProjectOption[]>;
  workItemTypes?: (input: { workspaceSlug: string; projectId: string }) => Promise<WorkItemTypeOption[]>;
  detailBadges?: DetailBadgeSlot;
  issueTypeDetail?: IssueTypeDetailSlot;
}

let providers: EditionIssuesProviders = {};

/** Register (or replace) the cloud providers. Called once at bootstrap. */
export function registerEditionIssuesProviders(next: EditionIssuesProviders | null): void {
  providers = next ?? {};
}

/** The registered providers; empty in the community build. */
export function getEditionIssuesProviders(): EditionIssuesProviders {
  return providers;
}

/** For tests: forget any registration. */
export function resetEditionIssuesProviders(): void {
  providers = {};
}

/** The states a transition gate may pick as a destination. */
export type GatedState = Pick<State, "id" | "name" | "group">;
