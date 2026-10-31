// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Work-item contextual palette entries (shell spec, SHELL-090). Pure
// selector: the mounting route assembles the subject, its capabilities
// and the resolved picker data, and this module returns the commands
// plus their sub-pages. Editing entries hide for guests, archived
// items and disabled project surfaces; the copy entries always show.
// Picks apply through the thin issue contracts (full ownership stays
// in the issues area); unchanged picks are no-ops.

import {
  ISSUE_PRIORITIES,
  orderEstimatePoints,
  selectActiveEstimate,
  type CycleList,
  type EstimateList,
  type IssuePatch,
  type LabelList,
  type StateList,
  type ModuleList,
} from "@pidash/api-client";

import type { PalettePage, PalettePageOption, PalettePageState, PidashCommand } from "../../commands/registry.js";
import type { NamedProjectMember } from "./api.js";
import { canEditInProject, CONTEXT_INDICATORS, CONTEXT_SECTIONS, type ProjectStanding } from "./types.js";

/** Resolved picker data behind the work-item sub-pages. */
export interface WorkItemPickerData {
  states: StateList;
  labels: LabelList;
  members: NamedProjectMember[];
  estimates: EstimateList;
  cycles: CycleList;
  modules: ModuleList;
  /** True while any picker list is still loading. */
  loading: boolean;
}

/** Actions the entries invoke (mutations + clipboard live with the route). */
export interface WorkItemContextActions {
  updateFields: (patch: IssuePatch) => void;
  addToCycle: (cycleId: string) => void;
  setModules: (input: { modules: string[]; removed_modules?: string[] }) => void;
  toggleSubscription: (subscribed: boolean) => void;
  openDelete: () => void;
  copyId: () => void;
  copyTitle: () => void;
  copyUrl: () => void;
  /**
   * Close the palette. Single-change pickers (state, priority,
   * estimate, cycle) call this on every pick; the multi-change
   * pickers (assignee, labels, modules) stay open across picks.
   */
  close: () => void;
}

export interface WorkItemContextInput {
  workspaceSlug: string;
  projectId: string;
  issueId: string;
  /** Route key shown in the chip (e.g. "PROJ-123"). */
  issueKey: string;
  stateId: string | null;
  priority: (typeof ISSUE_PRIORITIES)[number];
  assigneeIds: string[];
  labelIds: string[];
  estimatePoint: string | number | null | undefined;
  cycleId: string | null;
  moduleIds: string[];
  archived: boolean;
  /** Project surfaces gating the estimate/cycle/module pickers. */
  estimatesEnabled: boolean;
  cycleView: boolean;
  moduleView: boolean;
  meId: string | null;
  subscribed: boolean;
  standing: ProjectStanding;
  /** Whether this viewer may delete the item (creator or project admin, ISS-222). */
  mayDelete: boolean;
  pickers: WorkItemPickerData;
  actions: WorkItemContextActions;
}

export interface WorkItemContextSelection {
  commands: PidashCommand[];
  pages: PalettePage[];
}

const PRIORITY_TITLES: Record<(typeof ISSUE_PRIORITIES)[number], string> = {
  urgent: "Urgent",
  high: "High",
  medium: "Medium",
  low: "Low",
  none: "None",
};

/** Minutes render as hours + minutes on TIME estimate systems. */
export function formatEstimatePoint(value: string, system: string): string {
  if (system !== "time") return value;
  const minutes = Number(value);
  if (!Number.isFinite(minutes) || minutes < 0) return value;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return `${rest}m`;
  if (rest === 0) return `${hours}h`;
  return `${hours}h ${rest}m`;
}

function filterOptions(options: PalettePageOption[], query: string): PalettePageOption[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") return options;
  return options.filter((option) => option.title.toLowerCase().includes(needle));
}

function pageState(options: PalettePageOption[], emptyText: string, loading: boolean) {
  return (query: string): PalettePageState => {
    if (loading) return { status: "loading", options: [], emptyText };
    const matches = filterOptions(options, query);
    return { status: "ready", options: matches, emptyText };
  };
}

export function selectWorkItemContext(input: WorkItemContextInput): WorkItemContextSelection {
  const section = CONTEXT_SECTIONS["work-item"];
  const context = { kind: "work-item" as const, indicator: CONTEXT_INDICATORS["work-item"], subject: input.issueKey };
  const editable = canEditInProject(input.standing) && !input.archived;
  const assignedToMe = input.meId !== null && input.assigneeIds.includes(input.meId);
  const commands: PidashCommand[] = [];
  const pages: PalettePage[] = [];
  const prefix = `context.work-item.${input.issueId}`;

  function pageCommand(title: string, page: string, shortcut?: string): PidashCommand {
    return {
      id: `${prefix}.${page}`,
      title,
      section,
      page: `${prefix}.${page}`,
      shortcut,
      context,
      run: () => undefined,
    };
  }

  function actionCommand(title: string, name: string, run: () => void, shortcut?: string): PidashCommand {
    return { id: `${prefix}.${name}`, title, section, shortcut, context, run };
  }

  if (editable) {
    commands.push(pageCommand("Change state", "state", "s"));
    pages.push({
      id: `${prefix}.state`,
      placeholder: "Change state",
      usePageState: pageState(
        input.pickers.states.map((state) => ({
          id: state.id,
          title: state.name,
          selected: state.id === input.stateId,
          run: () => {
            if (state.id !== input.stateId) input.actions.updateFields({ state_id: state.id });
            input.actions.close();
          },
        })),
        "No states found",
        input.pickers.loading
      ),
    });

    commands.push(pageCommand("Change priority", "priority", "p"));
    pages.push({
      id: `${prefix}.priority`,
      placeholder: "Change priority",
      usePageState: pageState(
        ISSUE_PRIORITIES.map((priority) => ({
          id: priority,
          title: PRIORITY_TITLES[priority],
          selected: priority === input.priority,
          run: () => {
            if (priority !== input.priority) input.actions.updateFields({ priority });
            input.actions.close();
          },
        })),
        "No priorities found",
        false
      ),
    });

    commands.push(pageCommand("Assign to", "assignee", "a"));
    pages.push({
      id: `${prefix}.assignee`,
      placeholder: "Assign to",
      usePageState: pageState(
        input.pickers.members.map((member) => ({
          id: member.userId,
          title: member.displayName,
          selected: input.assigneeIds.includes(member.userId),
          run: () => {
            const next = input.assigneeIds.includes(member.userId)
              ? input.assigneeIds.filter((id) => id !== member.userId)
              : [...input.assigneeIds, member.userId];
            input.actions.updateFields({ assignee_ids: next });
          },
        })),
        "No members found",
        input.pickers.loading
      ),
    });

    commands.push(
      actionCommand(
        assignedToMe ? "Un-assign from me" : "Assign to me",
        "assign-self",
        () => {
          if (input.meId === null) return;
          input.actions.updateFields({
            assignee_ids: assignedToMe
              ? input.assigneeIds.filter((id) => id !== input.meId)
              : [...input.assigneeIds, input.meId],
          });
        },
        "i"
      )
    );

    if (input.estimatesEnabled) {
      commands.push(pageCommand("Change estimate", "estimate", "shift+e"));
      const active = selectActiveEstimate(input.pickers.estimates);
      const current =
        input.estimatePoint === undefined || input.estimatePoint === null ? null : String(input.estimatePoint);
      const pointOptions: PalettePageOption[] = orderEstimatePoints(active?.points ?? []).map((point) => ({
        id: point.id,
        title: formatEstimatePoint(point.value, active?.type ?? ""),
        selected: point.id === current,
        run: () => {
          if (point.id !== current) input.actions.updateFields({ estimate_point: point.id });
          input.actions.close();
        },
      }));
      pointOptions.unshift({
        id: "none",
        title: "No estimate",
        selected: current === null,
        run: () => {
          if (current !== null) input.actions.updateFields({ estimate_point: null });
          input.actions.close();
        },
      });
      pages.push({
        id: `${prefix}.estimate`,
        placeholder: "Change estimate",
        usePageState: pageState(pointOptions, "No estimates found", input.pickers.loading),
      });
    }

    if (input.cycleView) {
      commands.push(pageCommand("Add to cycle", "cycle", "shift+c"));
      pages.push({
        id: `${prefix}.cycle`,
        placeholder: "Add to cycle",
        usePageState: pageState(
          input.pickers.cycles.map((cycle) => ({
            id: cycle.id,
            title: cycle.name,
            selected: cycle.id === input.cycleId,
            run: () => {
              if (cycle.id !== input.cycleId) input.actions.addToCycle(cycle.id);
              input.actions.close();
            },
          })),
          "No cycles found",
          input.pickers.loading
        ),
      });
    }

    if (input.moduleView) {
      commands.push(pageCommand("Add to modules", "modules", "shift+m"));
      pages.push({
        id: `${prefix}.modules`,
        placeholder: "Add to modules",
        usePageState: pageState(
          input.pickers.modules.map((module) => ({
            id: module.id,
            title: module.name,
            selected: input.moduleIds.includes(module.id),
            run: () => {
              input.actions.setModules(
                input.moduleIds.includes(module.id)
                  ? { modules: [], removed_modules: [module.id] }
                  : { modules: [...input.moduleIds, module.id] }
              );
            },
          })),
          "No modules found",
          input.pickers.loading
        ),
      });
    }

    commands.push(pageCommand("Add labels", "labels", "l"));
    pages.push({
      id: `${prefix}.labels`,
      placeholder: "Add labels",
      usePageState: pageState(
        input.pickers.labels.map((label) => ({
          id: label.id,
          title: label.name,
          selected: input.labelIds.includes(label.id),
          run: () => {
            input.actions.updateFields({
              label_ids: input.labelIds.includes(label.id)
                ? input.labelIds.filter((id) => id !== label.id)
                : [...input.labelIds, label.id],
            });
          },
        })),
        "No labels found",
        input.pickers.loading
      ),
    });

    commands.push(
      actionCommand(
        input.subscribed ? "Unsubscribe from notifications" : "Subscribe to notifications",
        "subscription",
        () => input.actions.toggleSubscription(input.subscribed),
        "shift+s"
      )
    );

    if (input.mayDelete) {
      commands.push(actionCommand("Delete", "delete", () => input.actions.openDelete(), "mod+backspace"));
    }
  }

  commands.push(actionCommand("Copy ID", "copy-id", () => input.actions.copyId(), "mod+."));
  commands.push(actionCommand("Copy title", "copy-title", () => input.actions.copyTitle(), "mod+shift+'"));
  commands.push(actionCommand("Copy URL", "copy-url", () => input.actions.copyUrl(), "mod+shift+,"));

  return { commands, pages };
}
