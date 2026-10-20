// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace palette wiring (shell spec §8, SHELL-086-089/092).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { clearCommands, clearPalettePages, findPalettePage, listCommands } from "../../commands/index.js";
import { WorkspacePaletteEntries, type WorkspacePaletteEntriesProps } from "./WorkspaceEntries.js";

const responded = vi.hoisted(() => ({ role: 15, creationDisabled: false }));

function props(overrides: Partial<WorkspacePaletteEntriesProps> = {}): WorkspacePaletteEntriesProps {
  return {
    workspaceSlug: "acme",
    workspaceName: "Acme",
    project: null,
    projects: [{ id: "p1", name: "Atlas", identifier: "ATL" }],
    workspaces: [{ id: "w1", name: "Acme", slug: "acme" }],
    onOpenCreateProject: () => undefined,
    onCreateWorkspace: () => undefined,
    onNavigate: () => undefined,
    onWorkspaceInvites: () => undefined,
    onOpenExternal: () => undefined,
    onSelectWorkspace: () => undefined,
    onSelectProject: () => undefined,
    ...overrides,
  };
}

function renderEntries(entryProps: WorkspacePaletteEntriesProps = props()) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={queryClient}>
      <WorkspacePaletteEntries {...entryProps} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  responded.role = 15;
  responded.creationDisabled = false;
  createAppClient("https://api.test", (async (url: string) => {
    if (url.endsWith("/api/users/me/workspaces/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify([
            {
              id: "123e4567-e89b-12d3-a456-426614174000",
              name: "Acme",
              slug: "acme",
              logo_url: null,
              total_members: 1,
              role: responded.role,
            },
          ]),
      };
    }
    if (url.endsWith("/api/instances/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify({
            config: {
              enable_signup: true,
              is_workspace_creation_disabled: responded.creationDisabled,
              is_google_enabled: false,
              is_github_enabled: false,
              is_gitlab_enabled: false,
              is_gitea_enabled: false,
              is_magic_login_enabled: false,
              is_email_password_enabled: true,
              is_smtp_configured: false,
              is_self_managed: true,
              instance_changelog_url: "",
            },
          }),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
});

afterEach(() => {
  resetAppClient();
  clearCommands();
  clearPalettePages();
});

describe("WorkspacePaletteEntries", () => {
  it("registers the catalogs and picker pages", async () => {
    renderEntries();
    await waitFor(() =>
      expect(listCommands().map((command) => command.id)).toEqual([
        "shell.create-project",
        "shell.create-workspace",
        "shell.go-home",
        "shell.go-projects",
        "shell.go-archives",
        "shell.go-drafts",
        "shell.open-project",
        "shell.open-workspace",
        "shell.workspace-invites",
        "shell.copy-page-url",
        "shell.focus-search",
        "shell.toggle-sidebar",
        "shell.change-first-day",
        "shell.change-language",
        "shell.change-theme",
        "shell.change-timezone",
        "shell.forum",
        "shell.open-shortcuts",
        "shell.docs",
        "shell.report-bug",
      ])
    );
    expect(findPalettePage("shell.project-picker")).toBeDefined();
    expect(findPalettePage("shell.workspace-picker")).toBeDefined();
    expect(findPalettePage("shell.theme-picker")).toBeDefined();
    expect(findPalettePage("shell.timezone-picker")).toBeDefined();
    expect(findPalettePage("shell.first-day-picker")).toBeDefined();
    expect(findPalettePage("shell.language-picker")).toBeDefined();
  });

  it("hides create-gated entries without rights", async () => {
    responded.role = 5;
    responded.creationDisabled = true;
    renderEntries();
    await waitFor(() => expect(listCommands().map((command) => command.id)).toContain("shell.go-home"));
    await waitFor(() =>
      expect(listCommands().map((command) => command.id)).toEqual([
        "shell.go-home",
        "shell.go-projects",
        "shell.open-project",
        "shell.open-workspace",
        "shell.workspace-invites",
        "shell.copy-page-url",
        "shell.focus-search",
        "shell.toggle-sidebar",
        "shell.change-first-day",
        "shell.change-language",
        "shell.change-theme",
        "shell.change-timezone",
        "shell.forum",
        "shell.open-shortcuts",
        "shell.docs",
        "shell.report-bug",
      ])
    );
  });

  it("unregisters on unmount", () => {
    const entry = renderEntries();
    expect(listCommands()).not.toHaveLength(0);
    entry.unmount();
    expect(listCommands()).toHaveLength(0);
    expect(findPalettePage("shell.project-picker")).toBeUndefined();
  });

  it("wires runs through to the callbacks", async () => {
    const onNavigate = vi.fn();
    const onOpenCreateProject = vi.fn();
    renderEntries(props({ onNavigate, onOpenCreateProject }));
    await waitFor(() => expect(listCommands().map((command) => command.id)).toContain("shell.create-project"));
    const byId = new Map(listCommands().map((command) => [command.id, command]));
    byId.get("shell.create-project")!.run();
    expect(onOpenCreateProject).toHaveBeenCalledTimes(1);
    byId.get("shell.go-home")!.run();
    expect(onNavigate).toHaveBeenCalledWith("/acme");
  });
});
