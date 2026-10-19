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
import { getToastManager } from "../toaster.js";
import {
  selectFavoriteId,
  selectNamedProjectMembers,
  useCycleModuleMutations,
  usePageMutations,
  usePaletteCycles,
  usePaletteEstimates,
  usePaletteFavorites,
  usePaletteLabels,
  usePaletteModules,
  usePaletteProjectMembers,
  usePaletteStates,
  usePaletteSubscription,
  usePaletteWorkspaceMembers,
  useWorkItemMutations,
} from "./api.js";

const UUID = "123e4567-e89b-12d3-a456-426614174000";
const UUID2 = "223e4567-e89b-12d3-a456-426614174001";

function json(url: string, body: string, status = 200) {
  return { status, url, headers: { get: () => "application/json" }, text: async () => body };
}

interface StubRoute {
  status: number;
  body: string;
}

function harness(routes: (url: string, method: string) => StubRoute, failures: string[] = []) {
  const calls: Array<{ url: string; method: string; body?: string | undefined }> = [];
  const transport = async (url: string, init: { method: string; body?: string | undefined }) => {
    calls.push({ url, method: init.method, body: init.body });
    if (url.endsWith("/auth/get-csrf-token/")) return json(url, '{"csrf_token":"csrf"}');
    if (failures.some((marker) => url.includes(marker))) return json(url, '{"detail":"boom"}', 500);
    const route = routes(url, init.method);
    return json(url, route.body, route.status);
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return { queryClient, calls };
}

const STATE = {
  id: UUID,
  project_id: UUID2,
  workspace_id: UUID2,
  name: "Triage",
  color: "#ff0000",
  group: "unstarted",
  default: true,
  description: "",
  sequence: 1,
};
const LABEL = {
  id: UUID,
  project_id: UUID2,
  workspace_id: UUID2,
  name: "Bug",
  color: "#ff0000",
  parent: null,
  sort_order: 1,
};
const PROJECT_MEMBER = {
  id: UUID,
  role: 15,
  member: UUID2,
  project: UUID2,
  original_role: 15,
  created_at: "2026-01-01T00:00:00Z",
};
const WORKSPACE_MEMBER = {
  id: UUID,
  member: {
    id: UUID2,
    first_name: "Ada",
    last_name: "L",
    avatar: "",
    avatar_url: null,
    display_name: "Ada L",
    is_bot: false,
  },
  role: 15,
};
const ESTIMATE = {
  id: UUID,
  workspace: UUID2,
  project: UUID2,
  name: "Points",
  description: "",
  type: "points",
  last_used: true,
  points: [{ id: UUID2, key: 1, value: "1", description: "" }],
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
  created_by: null,
  updated_by: null,
};
const CYCLE = { id: UUID, project_id: UUID2, name: "Sprint 1", is_favorite: false };
const MODULE = { id: UUID, project_id: UUID2, name: "Auth", status: "backlog", is_favorite: false };

function referenceRoutes(url: string, method: string): StubRoute {
  if (method !== "GET") return mutationRoutes(url, method);
  if (url.includes("/states/")) return { status: 200, body: JSON.stringify([STATE]) };
  if (url.includes("/issue-labels/")) return { status: 200, body: JSON.stringify([LABEL]) };
  if (url.includes("/projects/") && url.includes("/members/"))
    return { status: 200, body: JSON.stringify([PROJECT_MEMBER]) };
  if (url.includes("/members/")) return { status: 200, body: JSON.stringify([WORKSPACE_MEMBER]) };
  if (url.includes("/estimates/")) return { status: 200, body: JSON.stringify([ESTIMATE]) };
  if (url.endsWith("/cycles/")) return { status: 200, body: JSON.stringify([CYCLE]) };
  if (url.endsWith("/modules/")) return { status: 200, body: JSON.stringify([MODULE]) };
  if (url.includes("/subscribe/")) return { status: 200, body: JSON.stringify({ subscribed: false }) };
  if (url.includes("/user-favorites/")) return { status: 200, body: JSON.stringify([]) };
  throw new Error(`unexpected call ${method} ${url}`);
}

function mutationRoutes(url: string, method: string): StubRoute {
  if (url.includes("/subscribe/")) {
    if (method === "POST") return { status: 200, body: JSON.stringify({ id: UUID, subscriber: UUID2 }) };
    return { status: 204, body: "" };
  }
  if (url.includes("/user-favorites/")) {
    if (method === "POST")
      return {
        status: 200,
        body: JSON.stringify({
          id: UUID,
          entity_type: "cycle",
          entity_identifier: UUID,
          entity_data: {},
          name: null,
          is_folder: false,
          sequence: 1,
          parent: null,
          workspace_id: UUID2,
          project_id: UUID2,
        }),
      };
    return { status: 204, body: "" };
  }
  if (method === "PATCH" && url.includes("/issues/")) return { status: 204, body: "" };
  if (method === "DELETE" && url.includes("/issues/")) return { status: 204, body: "" };
  if (url.includes("/cycle-issues/")) {
    if (method === "POST") return { status: 201, body: JSON.stringify({ message: "success" }) };
    return { status: 204, body: "" };
  }
  if (url.includes("/issues/") && url.includes("/modules/"))
    return { status: 201, body: JSON.stringify({ message: "success" }) };
  if (method === "PATCH" && url.includes("/modules/"))
    return {
      status: 200,
      body: JSON.stringify({ id: UUID, name: "Auth", project_id: UUID2, start_date: null, target_date: null }),
    };
  if (url.includes("/lock/") || url.includes("/access/")) return { status: 204, body: "" };
  if (url.includes("/archive/")) {
    if (method === "POST") return { status: 200, body: JSON.stringify({ archived_at: "2026-10-15T00:00:00Z" }) };
    return { status: 204, body: "" };
  }
  if (url.includes("/favorite-pages/")) return { status: 204, body: "" };
  throw new Error(`unexpected call ${method} ${url}`);
}

describe("contextual selectors", () => {
  it("joins project members to display names with an unknown fallback", () => {
    const named = selectNamedProjectMembers([PROJECT_MEMBER], [WORKSPACE_MEMBER]);
    expect(named).toEqual([{ userId: UUID2, role: 15, displayName: "Ada L" }]);
    const unknown = selectNamedProjectMembers([PROJECT_MEMBER], []);
    expect(unknown).toEqual([{ userId: UUID2, role: 15, displayName: "Unknown member" }]);
  });

  it("finds the favorite row id per entity family", () => {
    const rows = [
      { id: "fav-1", entity_type: "cycle", entity_identifier: UUID } as never,
      { id: "fav-2", entity_type: "module", entity_identifier: UUID2 } as never,
    ];
    expect(selectFavoriteId(rows, "cycle", UUID)).toBe("fav-1");
    expect(selectFavoriteId(rows, "module", UUID2)).toBe("fav-2");
    expect(selectFavoriteId(rows, "cycle", UUID2)).toBeNull();
    expect(selectFavoriteId([], "cycle", UUID)).toBeNull();
  });
});

describe("contextual reference hooks", () => {
  afterEach(() => {
    resetAppClient();
  });

  function ReferenceProbe(): React.ReactElement {
    const states = usePaletteStates("acme", "project-1");
    const labels = usePaletteLabels("acme", "project-1");
    const projectMembers = usePaletteProjectMembers("acme", "project-1");
    const workspaceMembers = usePaletteWorkspaceMembers("acme");
    const estimates = usePaletteEstimates("acme", "project-1");
    const cycles = usePaletteCycles("acme", "project-1");
    const modules = usePaletteModules("acme", "project-1");
    const subscription = usePaletteSubscription("acme", "project-1", "issue-1");
    const favorites = usePaletteFavorites("acme");
    const rows = [
      states.data?.length ?? -1,
      labels.data?.length ?? -1,
      projectMembers.data?.length ?? -1,
      workspaceMembers.data?.length ?? -1,
      estimates.data?.length ?? -1,
      cycles.data?.length ?? -1,
      modules.data?.length ?? -1,
      subscription.data === undefined ? -1 : 1,
      favorites.data?.length ?? -1,
    ];
    return <span data-testid="counts">{rows.join(",")}</span>;
  }

  it("loads every picker list through the app client", async () => {
    const { queryClient, calls } = harness(referenceRoutes);
    render(
      <QueryClientProvider client={queryClient}>
        <ReferenceProbe />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByTestId("counts")).toHaveTextContent("1,1,1,1,1,1,1,1,0"));
    expect(calls.some((call) => call.url.includes("/states/"))).toBe(true);
    expect(calls.some((call) => call.url.includes("/estimates/"))).toBe(true);
  });
});

describe("useWorkItemMutations", () => {
  afterEach(() => {
    resetAppClient();
  });

  function WorkItemProbe({ refresh }: { refresh: () => void }): React.ReactElement {
    const mutations = useWorkItemMutations({
      workspaceSlug: "acme",
      projectId: "project-1",
      issueId: "issue-1",
      noun: "Work item",
      refresh: async () => refresh(),
    });
    return (
      <div>
        <button type="button" onClick={() => mutations.updateFields.mutate({ priority: "high" })}>
          update
        </button>
        <button type="button" onClick={() => mutations.addToCycle.mutate("cycle-1")}>
          add-cycle
        </button>
        <button type="button" onClick={() => mutations.setModules.mutate({ modules: ["module-1"] })}>
          set-modules
        </button>
        <button type="button" onClick={() => mutations.toggleSubscription.mutate(false)}>
          subscribe
        </button>
        <button type="button" onClick={() => mutations.toggleSubscription.mutate(true)}>
          unsubscribe
        </button>
        <button type="button" onClick={() => mutations.remove.mutate()}>
          delete
        </button>
      </div>
    );
  }

  async function click(name: string): Promise<void> {
    const user = userEvent.setup();
    await act(async () => {
      await user.click(screen.getByRole("button", { name }));
    });
  }

  it("writes fields, memberships and subscription with refresh", async () => {
    const refresh = vi.fn();
    const { queryClient, calls } = harness(referenceRoutes);
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <WorkItemProbe refresh={refresh} />
      </QueryClientProvider>
    );
    await click("update");
    await click("add-cycle");
    await click("set-modules");
    await click("subscribe");
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(4));
    expect(calls.some((call) => call.method === "PATCH" && call.url.includes("/issues/"))).toBe(true);
    expect(calls.some((call) => call.method === "POST" && call.url.includes("/cycle-issues/"))).toBe(true);
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/modules/"))).toBe(true);
    expect(await screen.findByText("Subscribed to work item notifications.")).toBeInTheDocument();
    await click("unsubscribe");
    expect(await screen.findByText("Unsubscribed from work item notifications.")).toBeInTheDocument();
  });

  it("toasts update failures", async () => {
    const refresh = vi.fn();
    const { queryClient, calls } = harness(referenceRoutes, ["/issues/", "/cycle-issues/", "/subscribe/"]);
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <WorkItemProbe refresh={refresh} />
      </QueryClientProvider>
    );
    await click("update");
    expect(await screen.findByText("Couldn't update the work item.")).toBeInTheDocument();
    await click("add-cycle");
    await click("set-modules");
    await waitFor(() =>
      expect(calls.filter((call) => call.method === "POST" && call.url.includes("/cycle-issues/"))).not.toHaveLength(0)
    );
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/modules/"))).toBe(true);
    expect(await screen.findAllByText("Couldn't update the work item.")).not.toHaveLength(0);
    await click("subscribe");
    expect(await screen.findByText("Couldn't update the subscription.")).toBeInTheDocument();
    expect(refresh).not.toHaveBeenCalled();
  });

  it("deletes with the noun toast and permission wording on 403", async () => {
    const refresh = vi.fn();
    const transport = async (url: string, init: { method: string }) => {
      if (url.endsWith("/auth/get-csrf-token/")) return json(url, '{"csrf_token":"csrf"}');
      if (init.method === "DELETE") return json(url, '{"detail":"forbidden"}', 403);
      return json(url, "{}");
    };
    createAppClient("https://api.test", transport as never);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <WorkItemProbe refresh={refresh} />
      </QueryClientProvider>
    );
    await click("delete");
    expect(
      await screen.findByText("Only the creator or a project admin can delete this work item.")
    ).toBeInTheDocument();
  });

  it("toasts generic delete failures", async () => {
    const refresh = vi.fn();
    const { queryClient } = harness(referenceRoutes, ["/issues/issue-1/"]);
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <WorkItemProbe refresh={refresh} />
      </QueryClientProvider>
    );
    await click("delete");
    expect(await screen.findByText("Couldn't delete the work item.")).toBeInTheDocument();
  });
});

describe("useCycleModuleMutations", () => {
  afterEach(() => {
    resetAppClient();
  });

  function CycleModuleProbe({ refresh }: { refresh: () => void }): React.ReactElement {
    const mutations = useCycleModuleMutations({
      workspaceSlug: "acme",
      projectId: "project-1",
      refresh: async () => refresh(),
    });
    return (
      <div>
        <button
          type="button"
          onClick={() =>
            mutations.toggleFavorite.mutate({ entityType: "cycle", entityId: "cycle-1", favoriteId: null })
          }
        >
          star
        </button>
        <button
          type="button"
          onClick={() =>
            mutations.toggleFavorite.mutate({ entityType: "cycle", entityId: "cycle-1", favoriteId: "fav-1" })
          }
        >
          unstar
        </button>
        <button
          type="button"
          onClick={() => mutations.updateModuleFields.mutate({ moduleId: "module-1", patch: { status: "planned" } })}
        >
          status
        </button>
      </div>
    );
  }

  it("toggles favorites and module fields", async () => {
    const user = userEvent.setup();
    const refresh = vi.fn();
    const { queryClient, calls } = harness(referenceRoutes);
    render(
      <QueryClientProvider client={queryClient}>
        <CycleModuleProbe refresh={refresh} />
      </QueryClientProvider>
    );
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "star" }));
    });
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "unstar" }));
    });
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "status" }));
    });
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(3));
    expect(calls.some((call) => call.method === "POST" && call.url.includes("/user-favorites/"))).toBe(true);
    expect(calls.some((call) => call.method === "DELETE" && call.url.includes("/user-favorites/"))).toBe(true);
    expect(calls.some((call) => call.method === "PATCH" && call.url.includes("/modules/"))).toBe(true);
  });

  it("toasts favorite and module failures", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(referenceRoutes, ["/user-favorites/", "/modules/"]);
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <CycleModuleProbe refresh={() => undefined} />
      </QueryClientProvider>
    );
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "star" }));
    });
    expect(await screen.findByText("Couldn't update favorites.")).toBeInTheDocument();
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "status" }));
    });
    expect(await screen.findByText("Couldn't update the module.")).toBeInTheDocument();
  });
});

describe("usePageMutations", () => {
  afterEach(() => {
    resetAppClient();
  });

  function PageProbe({ refresh }: { refresh: () => void }): React.ReactElement {
    const mutations = usePageMutations({
      workspaceSlug: "acme",
      projectId: "project-1",
      refresh: async () => refresh(),
    });
    return (
      <div>
        <button type="button" onClick={() => mutations.setLocked.mutate({ pageId: "page-1", locked: true })}>
          lock
        </button>
        <button type="button" onClick={() => mutations.setLocked.mutate({ pageId: "page-1", locked: false })}>
          unlock
        </button>
        <button type="button" onClick={() => mutations.setAccess.mutate({ pageId: "page-1", access: 1 })}>
          access
        </button>
        <button type="button" onClick={() => mutations.setArchived.mutate({ pageId: "page-1", archived: true })}>
          archive
        </button>
        <button type="button" onClick={() => mutations.setArchived.mutate({ pageId: "page-1", archived: false })}>
          restore
        </button>
        <button type="button" onClick={() => mutations.toggleFavorite.mutate({ pageId: "page-1", favorite: true })}>
          star
        </button>
        <button type="button" onClick={() => mutations.toggleFavorite.mutate({ pageId: "page-1", favorite: false })}>
          unstar
        </button>
      </div>
    );
  }

  it("drives every page write", async () => {
    const user = userEvent.setup();
    const refresh = vi.fn();
    const { queryClient, calls } = harness(referenceRoutes);
    render(
      <QueryClientProvider client={queryClient}>
        <PageProbe refresh={refresh} />
      </QueryClientProvider>
    );
    for (const name of ["lock", "unlock", "access", "archive", "restore", "star", "unstar"]) {
      await act(async () => {
        await user.click(screen.getByRole("button", { name }));
      });
    }
    await waitFor(() => expect(refresh).toHaveBeenCalledTimes(7));
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/lock/"))).toBe(true);
    expect(calls.some((call) => call.method === "DELETE" && call.url.endsWith("/lock/"))).toBe(true);
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/access/"))).toBe(true);
    expect(calls.some((call) => call.method === "POST" && call.url.endsWith("/archive/"))).toBe(true);
    expect(calls.some((call) => call.method === "DELETE" && call.url.endsWith("/archive/"))).toBe(true);
    expect(calls.some((call) => call.url.includes("/favorite-pages/"))).toBe(true);
  });

  it("toasts page write failures", async () => {
    const user = userEvent.setup();
    const { queryClient } = harness(referenceRoutes, ["/lock/", "/access/", "/archive/", "/favorite-pages/"]);
    render(
      <QueryClientProvider client={queryClient}>
        <ToastHost manager={getToastManager()} />
        <PageProbe refresh={() => undefined} />
      </QueryClientProvider>
    );
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "lock" }));
    });
    expect(await screen.findByText("Couldn't update the page lock.")).toBeInTheDocument();
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "access" }));
    });
    expect(await screen.findByText("Couldn't change page access.")).toBeInTheDocument();
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "archive" }));
    });
    expect(await screen.findByText("Couldn't update the page archive.")).toBeInTheDocument();
    await act(async () => {
      await user.click(screen.getByRole("button", { name: "star" }));
    });
    expect(await screen.findByText("Couldn't update favorites.")).toBeInTheDocument();
  });
});
