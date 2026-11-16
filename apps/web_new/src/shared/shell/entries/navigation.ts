// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette navigation entries (shell spec, SHELL-087). Direct jumps land on
// their destination; the project and workspace entries open filterable
// entity pickers. Only destinations that exist in this app register —
// inbox, analytics, drafts, views and settings entries arrive with the
// areas that own their routes, through the same registry. Gating hides
// entries entirely instead of disabling them (SHELL-092).

import type { PidashCommand } from "../../commands/index.js";

/** Project tab flags behind the project-level entries (SHELL-087). */
export interface NavigationProjectViews {
  cycleView: boolean;
  moduleView: boolean;
  issueViewsView: boolean;
  pageView: boolean;
  inboxView: boolean;
}

/** The project in context, or null outside one. */
export interface NavigationProject {
  id: string;
  name: string;
  views: NavigationProjectViews;
}

export interface NavigationEntryContext {
  workspaceSlug: string;
  workspaceName: string;
  /** Null outside a project; project-level entries stay hidden then. */
  project: NavigationProject | null;
  /** Workspace admin/member: may use the member-gated destinations. */
  canEdit: boolean;
  /** Client-side navigation to an app href. */
  navigate: (href: string) => void;
}

/** Sub-page id behind "Open a project" (SHELL-087). */
export const PROJECT_PICKER_PAGE_ID = "shell.project-picker";
/** Sub-page id behind "Open a workspace" (SHELL-087). */
export const WORKSPACE_PICKER_PAGE_ID = "shell.workspace-picker";

interface Jump {
  id: string;
  title: string;
  sequence: string;
  href: string;
  hint: string;
}

function jump(context: NavigationEntryContext, entry: Jump): PidashCommand {
  return {
    id: entry.id,
    title: entry.title,
    hint: entry.hint,
    section: "Navigate",
    sequence: entry.sequence,
    run: () => context.navigate(entry.href),
  };
}

/**
 * The navigation entries for the current context. Workspace-level jumps
 * and both pickers always register; project-level jumps need the project
 * context plus their tab flag, and the member-gated ones need canEdit.
 */
export function buildNavigationCommands(context: NavigationEntryContext): PidashCommand[] {
  const ws = `/${context.workspaceSlug}`;
  const commands: PidashCommand[] = [
    {
      id: "shell.open-project",
      title: "Open a project",
      hint: context.workspaceName,
      section: "Navigate",
      sequence: "op",
      page: PROJECT_PICKER_PAGE_ID,
      run: () => undefined,
    },
    {
      id: "shell.open-workspace",
      title: "Open a workspace",
      section: "Navigate",
      sequence: "ow",
      page: WORKSPACE_PICKER_PAGE_ID,
      run: () => undefined,
    },
    jump(context, {
      id: "shell.go-home",
      title: "Go to home",
      sequence: "gh",
      href: ws,
      hint: context.workspaceName,
    }),
    jump(context, {
      id: "shell.go-projects",
      title: "Go to projects list",
      sequence: "gp",
      href: `${ws}/projects`,
      hint: context.workspaceName,
    }),
  ];
  if (context.project === null) {
    if (context.canEdit) {
      // Both destinations belong to member-or-better viewers: the old
      // palette offers workspace drafts to members and the archives jump
      // no lower (SHELL-087, ISS-137).
      commands.push(
        jump(context, {
          id: "shell.go-drafts",
          title: "Go to workspace drafts",
          sequence: "gj",
          href: `${ws}/drafts`,
          hint: context.workspaceName,
        })
      );
      commands.push(
        jump(context, {
          id: "shell.go-archives",
          title: "Go to workspace archives",
          sequence: "gr",
          href: `${ws}/projects/archives`,
          hint: context.workspaceName,
        })
      );
    }
    return commands;
  }
  const project = context.project;
  const base = `${ws}/projects/${project.id}`;
  commands.push(
    jump(context, {
      id: "shell.go-work-items",
      title: "Go to work items",
      sequence: "gi",
      href: `${base}/issues`,
      hint: project.name,
    })
  );
  if (project.views.pageView) {
    commands.push(
      jump(context, {
        id: "shell.go-pages",
        title: "Go to pages",
        sequence: "gd",
        href: `${base}/pages`,
        hint: project.name,
      })
    );
  }
  if (context.canEdit && project.views.cycleView) {
    commands.push(
      jump(context, {
        id: "shell.go-cycles",
        title: "Go to cycles",
        sequence: "gc",
        href: `${base}/cycles`,
        hint: project.name,
      })
    );
  }
  if (context.canEdit && project.views.moduleView) {
    commands.push(
      jump(context, {
        id: "shell.go-modules",
        title: "Go to modules",
        sequence: "gm",
        href: `${base}/modules`,
        hint: project.name,
      })
    );
  }
  if (project.views.issueViewsView) {
    commands.push(
      jump(context, {
        id: "shell.go-views",
        title: "Go to project views",
        sequence: "gv",
        href: `${base}/views`,
        hint: project.name,
      })
    );
  }
  if (project.views.inboxView) {
    commands.push(
      jump(context, {
        id: "shell.go-intake",
        title: "Go to intake",
        sequence: "gk",
        href: `${base}/intake`,
        hint: project.name,
      })
    );
  }
  return commands;
}
