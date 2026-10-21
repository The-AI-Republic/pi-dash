// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ToastHost } from "@pidash/kit";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import {
  clearCommands,
  clearPalettePages,
  findCommand,
  findPalettePage,
  listCommands,
} from "../../commands/registry.js";
import { useCommandPalette } from "../../commands/store.js";
import { getToastManager } from "../toaster.js";
import {
  useCyclePaletteContext,
  useModulePaletteContext,
  usePagePaletteContext,
  useWorkItemPaletteContext,
} from "./hooks.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const ME_ID = "223e4567-e89b-12d3-a456-426614174001";
const FAV_ID = "323e4567-e89b-12d3-a456-426614174002";

function json(url: string, body: string, status = 200) {
  return { status, url, headers: { get: () => "application/json" }, text: async () => body };
}

const PROJECT_MEMBER = {
  id: UUID,
  role: 15,
  member: ME_ID,
  project: UUID,
  original_role: 15,
  created_at: "2026-01-01T00:00:00Z",
};
const WORKSPACE_MEMBER = {
  id: UUID,
  member: {
    id: ME_ID,
    first_name: "Ada",
    last_name: "L",
    avatar: "",
    avatar_url: null,
    display_name: "Ada L",
    is_bot: false,
  },
  role: 15,
};
const CYCLE = { id: UUID, project_id: UUID, name: "Sprint 1", is_favorite: false };
const MODULE = { id: UUID, project_id: UUID, name: "Auth", status: "backlog", is_favorite: true };
const MODULE_FAVORITE = {
  id: FAV_ID,
  entity_type: "module",
  entity_identifier: UUID,
  entity_data: {},
  name: null,
  is_folder: false,
  sequence: 1,
  parent: null,
  workspace_id: UUID,
  project_id: UUID,
};

function harness() {
  const calls: string[] = [];
  const transport = async (url: string, init: { method: string; body?: string | undefined }) => {
    calls.push(`${init.method} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) return json(url, '{"csrf_token":"csrf"}');
    if (init.method === "GET") {
      if (url.includes("/subscribe/")) return json(url, JSON.stringify({ subscribed: false }));
      if (url.includes("/user-favorites/")) return json(url, JSON.stringify([MODULE_FAVORITE]));
      if (url.includes("/projects/") && url.includes("/members/")) return json(url, JSON.stringify([PROJECT_MEMBER]));
      if (url.includes("/members/")) return json(url, JSON.stringify([WORKSPACE_MEMBER]));
      if (url.endsWith("/cycles/")) return json(url, JSON.stringify([CYCLE]));
      if (url.endsWith("/modules/")) return json(url, JSON.stringify([MODULE]));
      return json(url, JSON.stringify([]));
    }
    if (url.includes("/subscribe/")) return json(url, JSON.stringify({ id: UUID, subscriber: ME_ID }));
    if (url.includes("/user-favorites/") && init.method === "POST")
      return json(
        url,
        JSON.stringify({
          id: UUID,
          entity_type: "cycle",
          entity_identifier: UUID,
          entity_data: {},
          name: null,
          is_folder: false,
          sequence: 1,
          parent: null,
          workspace_id: UUID,
          project_id: UUID,
        })
      );
    if (url.includes("/archive/") && init.method === "POST")
      return json(url, JSON.stringify({ archived_at: "2026-10-15T00:00:00Z" }));
    if (init.method === "POST" && (url.includes("/cycle-issues/") || url.includes("/modules/")))
      return json(url, JSON.stringify({ message: "success" }), 201);
    if (init.method === "PATCH" && url.includes("/modules/"))
      return json(
        url,
        JSON.stringify({ id: UUID, name: "Auth", project_id: UUID, start_date: null, target_date: null })
      );
    return json(url, "", 204);
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return { queryClient, calls };
}

const WORK_ITEM_INPUT = {
  workspaceSlug: "acme",
  projectId: "project-1",
  issueId: "issue-1",
  issueKey: "ACME-12",
  issueName: "Fix the leak",
  stateId: null,
  priority: "medium" as const,
  assigneeIds: [] as string[],
  labelIds: [] as string[],
  estimatePoint: null,
  cycleId: null,
  moduleIds: [] as string[],
  isEpic: false,
  parentId: null,
  // (createdBy removed: the delete flow authorizes server-side via 403.)
  archived: false,
  estimatesEnabled: true,
  cycleView: true,
  moduleView: true,
  meId: ME_ID,
  standing: { memberRole: 15, archived: false },
  mayDelete: true,
};

function WorkItemHost({ onDeleted }: { onDeleted: () => void }): React.ReactElement {
  const binding = useWorkItemPaletteContext({ ...WORK_ITEM_INPUT, refresh: async () => undefined, onDeleted });
  return <div>{binding.deleteDialog}</div>;
}

describe("contextual mounting hooks", () => {
  afterEach(() => {
    resetAppClient();
    clearCommands();
    clearPalettePages();
    vi.unstubAllGlobals();
  });

  it("registers the work-item group while mounted and unregisters after", async () => {
    const { queryClient } = harness();
    const { unmount } = render(
      <QueryClientProvider client={queryClient}>
        <WorkItemHost onDeleted={() => undefined} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand("context.work-item.issue-1.state")).toBeDefined());
    expect(listCommands().filter((command) => command.section === "Work item actions")).toHaveLength(13);
    unmount();
    expect(listCommands()).toHaveLength(0);
  });

  it("opens the delete dialog, confirms and leaves the detail view", async () => {
    const user = userEvent.setup();
    const onDeleted = vi.fn();
    const { queryClient, calls } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <WorkItemHost onDeleted={onDeleted} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand("context.work-item.issue-1.delete")).toBeDefined());
    await act(async () => {
      await findCommand("context.work-item.issue-1.delete")!.run();
    });
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "Delete" }));
    });
    await waitFor(() => expect(onDeleted).toHaveBeenCalledTimes(1));
    expect(calls.some((call) => call.startsWith("DELETE") && call.includes("/issues/"))).toBe(true);
    expect(await screen.findByText("Work item deleted.")).toBeInTheDocument();
  });

  it("copies the work-item identity with confirmation toasts", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const { queryClient } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <WorkItemHost onDeleted={() => undefined} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand("context.work-item.issue-1.copy-id")).toBeDefined());
    await act(async () => {
      await findCommand("context.work-item.issue-1.copy-id")!.run();
      await findCommand("context.work-item.issue-1.copy-title")!.run();
      await findCommand("context.work-item.issue-1.copy-url")!.run();
    });
    expect(writeText).toHaveBeenCalledWith("ACME-12");
    expect(writeText).toHaveBeenCalledWith("Fix the leak");
    // DESK-019: the work-item entry copies the canonical browse address, not
    // whatever query the palette happens to sit behind.
    expect(writeText).toHaveBeenCalledWith(`${window.location.origin}/acme/browse/ACME-12/`);
    expect(await screen.findByText("Copied the work item ID.")).toBeInTheDocument();
    expect(user).toBeDefined();
  });

  it("reports copy failures when the clipboard is missing", async () => {
    vi.stubGlobal("navigator", {});
    const { queryClient } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <WorkItemHost onDeleted={() => undefined} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand("context.work-item.issue-1.copy-id")).toBeDefined());
    await act(async () => {
      await findCommand("context.work-item.issue-1.copy-id")!.run();
    });
    expect(await screen.findByText("Couldn't copy the work item ID.")).toBeInTheDocument();
  });

  it("registers cycle entries and toggles the favorite through user-favorites", async () => {
    function CycleHost(): React.ReactElement {
      useCyclePaletteContext({
        workspaceSlug: "acme",
        projectId: "project-1",
        cycleId: UUID,
        cycleName: "Sprint 1",
        archived: false,
        favorite: false,
        standing: { memberRole: 15, archived: false },
        refresh: async () => undefined,
      });
      return <div />;
    }
    const { queryClient, calls } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <CycleHost />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand(`context.cycle.${UUID}.favorite`)).toBeDefined());
    await act(async () => {
      await findCommand(`context.cycle.${UUID}.favorite`)!.run();
    });
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("POST") && call.includes("/user-favorites/"))).toBe(true)
    );
  });

  it("registers module entries and writes member and status picks", async () => {
    function ModuleHost(): React.ReactElement {
      useModulePaletteContext({
        workspaceSlug: "acme",
        projectId: "project-1",
        moduleId: UUID,
        moduleName: "Auth",
        archived: false,
        status: "backlog",
        memberIds: [],
        favorite: false,
        standing: { memberRole: 15, archived: false },
        refresh: async () => undefined,
      });
      return <div />;
    }
    const { queryClient, calls } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <ModuleHost />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand(`context.module.${UUID}.members`)).toBeDefined());
    expect(findCommand(`context.module.${UUID}.status`)).toBeDefined();
    expect(findCommand(`context.module.${UUID}.favorite`)).toBeDefined();
    expect(findCommand(`context.module.${UUID}.copy-url`)).toBeDefined();
    expect(calls.some((call) => call.includes("/projects/project-1/members/"))).toBe(true);
  });

  it("registers page entries and drives lock, access, archive and favorite", async () => {
    function PageHost(): React.ReactElement {
      usePagePaletteContext({
        workspaceSlug: "acme",
        projectId: "project-1",
        pageId: UUID,
        pageName: "Roadmap",
        locked: false,
        access: 0,
        archived: false,
        favorite: false,
        isOwner: true,
        highestProjectRole: 15,
        refresh: async () => undefined,
      });
      return <div />;
    }
    const { queryClient, calls } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <PageHost />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand(`context.page.${UUID}.lock`)).toBeDefined());
    await act(async () => {
      await findCommand(`context.page.${UUID}.lock`)!.run();
      await findCommand(`context.page.${UUID}.access`)!.run();
      await findCommand(`context.page.${UUID}.archive`)!.run();
      await findCommand(`context.page.${UUID}.favorite`)!.run();
    });
    await waitFor(() => expect(calls.some((call) => call.includes("/favorite-pages/"))).toBe(true));
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/lock/"))).toBe(true);
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/access/"))).toBe(true);
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/archive/"))).toBe(true);
  });

  it("applies work-item picks through the mutations and closes single-change picks", async () => {
    const { queryClient, calls } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <WorkItemHost onDeleted={() => undefined} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findPalettePage("context.work-item.issue-1.priority")).toBeDefined());
    await waitFor(() =>
      expect(findPalettePage("context.work-item.issue-1.cycle")!.usePageState("").options).toHaveLength(1)
    );
    await waitFor(() =>
      expect(findPalettePage("context.work-item.issue-1.modules")!.usePageState("").options).toHaveLength(1)
    );
    useCommandPalette.getState().openPalette();
    await act(async () => {
      findPalettePage("context.work-item.issue-1.priority")!
        .usePageState("")
        .options.find((option) => option.id === "high")!
        .run();
    });
    expect(useCommandPalette.getState().open).toBe(false);
    await act(async () => {
      await findCommand("context.work-item.issue-1.subscription")!.run();
      findPalettePage("context.work-item.issue-1.cycle")!.usePageState("").options[0]!.run();
      findPalettePage("context.work-item.issue-1.modules")!.usePageState("").options[0]!.run();
    });
    await waitFor(() => expect(calls.some((call) => call.startsWith("PATCH") && call.includes("/issues/"))).toBe(true));
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/subscribe/"))).toBe(true);
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/cycle-issues/"))).toBe(true);
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/issues/issue-1/modules/"))).toBe(true);
  });

  it("writes module member, status and favorite picks through the mounted hook", async () => {
    function ModuleHost({ memberIds }: { memberIds: string[] }): React.ReactElement {
      useModulePaletteContext({
        workspaceSlug: "acme",
        projectId: "project-1",
        moduleId: UUID,
        moduleName: "Auth",
        archived: false,
        status: "backlog",
        memberIds,
        favorite: true,
        standing: { memberRole: 15, archived: false },
        refresh: async () => undefined,
      });
      return <div />;
    }
    const { queryClient, calls } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <ModuleHost memberIds={[]} />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findPalettePage(`context.module.${UUID}.members`)).toBeDefined());
    await waitFor(() =>
      expect(findPalettePage(`context.module.${UUID}.members`)!.usePageState("").options).toHaveLength(1)
    );
    await act(async () => {
      findPalettePage(`context.module.${UUID}.members`)!.usePageState("").options[0]!.run();
    });
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("PATCH") && call.includes("/modules/"))).toBe(true)
    );
    useCommandPalette.getState().openPalette();
    await act(async () => {
      findPalettePage(`context.module.${UUID}.status`)!
        .usePageState("")
        .options.find((option) => option.id === "planned")!
        .run();
    });
    expect(useCommandPalette.getState().open).toBe(false);
    await act(async () => {
      await findCommand(`context.module.${UUID}.favorite`)!.run();
    });
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("DELETE") && call.includes("/user-favorites/"))).toBe(true)
    );
  });

  it("removes module members through the mounted hook", async () => {
    function ModuleHost(): React.ReactElement {
      useModulePaletteContext({
        workspaceSlug: "acme",
        projectId: "project-1",
        moduleId: UUID,
        moduleName: "Auth",
        archived: false,
        status: "backlog",
        memberIds: [ME_ID],
        favorite: false,
        standing: { memberRole: 15, archived: false },
        refresh: async () => undefined,
      });
      return <div />;
    }
    const { queryClient, calls } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <ModuleHost />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findPalettePage(`context.module.${UUID}.members`)).toBeDefined());
    await waitFor(() =>
      expect(findPalettePage(`context.module.${UUID}.members`)!.usePageState("").options).toHaveLength(1)
    );
    expect(findPalettePage(`context.module.${UUID}.members`)!.usePageState("").options[0]!.selected).toBe(true);
    await act(async () => {
      findPalettePage(`context.module.${UUID}.members`)!.usePageState("").options[0]!.run();
    });
    await waitFor(() =>
      expect(calls.some((call) => call.startsWith("PATCH") && call.includes("/modules/"))).toBe(true)
    );
  });

  it("copies cycle, module and page links with confirmation toasts", async () => {
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    function CycleHost(): React.ReactElement {
      useCyclePaletteContext({
        workspaceSlug: "acme",
        projectId: "project-1",
        cycleId: UUID,
        cycleName: "Sprint 1",
        archived: false,
        favorite: false,
        standing: { memberRole: 15, archived: false },
        refresh: async () => undefined,
      });
      return <div />;
    }
    function ModuleHost(): React.ReactElement {
      useModulePaletteContext({
        workspaceSlug: "acme",
        projectId: "project-1",
        moduleId: UUID,
        moduleName: "Auth",
        archived: false,
        status: "backlog",
        memberIds: [],
        favorite: false,
        standing: { memberRole: 15, archived: false },
        refresh: async () => undefined,
      });
      return <div />;
    }
    function PageHost(): React.ReactElement {
      usePagePaletteContext({
        workspaceSlug: "acme",
        projectId: "project-1",
        pageId: UUID,
        pageName: "Roadmap",
        locked: false,
        access: 0,
        archived: false,
        favorite: false,
        isOwner: true,
        highestProjectRole: 15,
        refresh: async () => undefined,
      });
      return <div />;
    }
    const { queryClient } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <CycleHost />
        <ModuleHost />
        <PageHost />
      </QueryClientProvider>
    );
    await waitFor(() => expect(findCommand(`context.cycle.${UUID}.copy-url`)).toBeDefined());
    await waitFor(() => expect(findCommand(`context.module.${UUID}.copy-url`)).toBeDefined());
    await waitFor(() => expect(findCommand(`context.page.${UUID}.copy-url`)).toBeDefined());
    await act(async () => {
      await findCommand(`context.cycle.${UUID}.copy-url`)!.run();
      await findCommand(`context.module.${UUID}.copy-url`)!.run();
      await findCommand(`context.page.${UUID}.copy-url`)!.run();
    });
    expect(writeText).toHaveBeenCalledTimes(3);
    expect(writeText).toHaveBeenCalledWith(window.location.href);
    expect(await screen.findByText("Copied the cycle link.")).toBeInTheDocument();
    expect(await screen.findByText("Copied the module link.")).toBeInTheDocument();
    expect(await screen.findByText("Copied the page link.")).toBeInTheDocument();
  });
});
