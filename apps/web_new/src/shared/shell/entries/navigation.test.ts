// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette navigation entries (shell spec, SHELL-087/092).
import { describe, expect, it, vi } from "vitest";
import {
  PROJECT_PICKER_PAGE_ID,
  WORKSPACE_PICKER_PAGE_ID,
  buildNavigationCommands,
  type NavigationEntryContext,
  type NavigationProject,
} from "./navigation.js";

function project(overrides: Partial<NavigationProject["views"]> = {}): NavigationProject {
  return {
    id: "project-id",
    name: "Website",
    views: {
      cycleView: true,
      moduleView: true,
      issueViewsView: true,
      pageView: true,
      inboxView: true,
      ...overrides,
    },
  };
}

function context(overrides: Partial<NavigationEntryContext> = {}): NavigationEntryContext {
  return {
    workspaceSlug: "acme",
    workspaceName: "Acme",
    project: null,
    canEdit: true,
    navigate: () => undefined,
    ...overrides,
  };
}

function idsOf(context: NavigationEntryContext): string[] {
  return buildNavigationCommands(context).map((command) => command.id);
}

describe("navigation entries", () => {
  it("registers pickers and workspace jumps outside a project", () => {
    const commands = buildNavigationCommands(context());
    expect(idsOf(context())).toEqual([
      "shell.open-project",
      "shell.open-workspace",
      "shell.go-home",
      "shell.go-projects",
      "shell.go-drafts",
      "shell.go-archives",
    ]);
    const picker = commands[0]!;
    expect(picker).toMatchObject({
      title: "Open a project",
      section: "Navigate",
      sequence: "op",
      page: PROJECT_PICKER_PAGE_ID,
    });
    expect(commands[1]).toMatchObject({
      title: "Open a workspace",
      section: "Navigate",
      sequence: "ow",
      page: WORKSPACE_PICKER_PAGE_ID,
    });
    expect(commands[2]).toMatchObject({ title: "Go to home", sequence: "gh", hint: "Acme" });
    expect(commands[4]).toMatchObject({ title: "Go to workspace drafts", sequence: "gj" });
    expect(commands[5]).toMatchObject({ title: "Go to workspace archives", sequence: "gr" });
  });

  it("hides the archives jump from viewers outside a project", () => {
    expect(idsOf(context({ canEdit: false }))).toEqual([
      "shell.open-project",
      "shell.open-workspace",
      "shell.go-home",
      "shell.go-projects",
    ]);
  });

  it("registers every project jump with all flags on", () => {
    expect(idsOf(context({ project: project() }))).toEqual([
      "shell.open-project",
      "shell.open-workspace",
      "shell.go-home",
      "shell.go-projects",
      "shell.go-work-items",
      "shell.go-pages",
      "shell.go-cycles",
      "shell.go-modules",
      "shell.go-views",
      "shell.go-intake",
    ]);
  });

  it("hides flag-gated jumps when their tab is off", () => {
    const entry = project({
      cycleView: false,
      moduleView: false,
      issueViewsView: false,
      pageView: false,
      inboxView: false,
    });
    expect(idsOf(context({ project: entry }))).toEqual([
      "shell.open-project",
      "shell.open-workspace",
      "shell.go-home",
      "shell.go-projects",
      "shell.go-work-items",
    ]);
  });

  it("hides the member-gated tab jumps from viewers in a project", () => {
    const entry = project();
    expect(idsOf(context({ project: entry, canEdit: false }))).toEqual([
      "shell.open-project",
      "shell.open-workspace",
      "shell.go-home",
      "shell.go-projects",
      "shell.go-work-items",
      "shell.go-pages",
      "shell.go-views",
      "shell.go-intake",
    ]);
  });

  it("navigates to hrefs on run", () => {
    const navigate = vi.fn();
    const commands = buildNavigationCommands(context({ project: project(), navigate }));
    const byId = new Map(commands.map((command) => [command.id, command]));
    byId.get("shell.go-home")!.run();
    byId.get("shell.go-projects")!.run();
    byId.get("shell.go-work-items")!.run();
    byId.get("shell.go-pages")!.run();
    byId.get("shell.go-cycles")!.run();
    byId.get("shell.go-modules")!.run();
    byId.get("shell.go-views")!.run();
    byId.get("shell.go-intake")!.run();
    expect(navigate.mock.calls.map((call) => call[0])).toEqual([
      "/acme",
      "/acme/projects",
      "/acme/projects/project-id/issues",
      "/acme/projects/project-id/pages",
      "/acme/projects/project-id/cycles",
      "/acme/projects/project-id/modules",
      "/acme/projects/project-id/views",
      "/acme/projects/project-id/intake",
    ]);
    const workspace = buildNavigationCommands(context({ navigate }));
    workspace.find((command) => command.id === "shell.go-drafts")!.run();
    expect(navigate).toHaveBeenLastCalledWith("/acme/drafts");
    workspace.find((command) => command.id === "shell.go-archives")!.run();
    expect(navigate).toHaveBeenLastCalledWith("/acme/projects/archives");
  });
});
