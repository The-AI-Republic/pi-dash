// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import type { ProjectDetails } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, notifyError: vi.fn(), notifySuccess: vi.fn() };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { FAVORITES_SECTION_OPEN_KEY } from "../../../shared/shell/favorites.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { projectKeys } from "./keys.js";
import {
  FAVORITE_ADDED_TITLE,
  FAVORITE_FAILED_TITLE,
  FAVORITE_REMOVED_TITLE,
  selectPendingProjectId,
  useToggleFavoriteProject,
  type ToggleFavoriteInput,
} from "./mutations.js";

const PROJECT_ID = "223e4567-e89b-12d3-a456-426614174001";
const WS_ID = "123e4567-e89b-12d3-a456-426614174000";

function project(overrides: Partial<ProjectDetails> = {}): ProjectDetails {
  return {
    id: PROJECT_ID,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    deleted_at: null,
    workspace: WS_ID,
    name: "Website relaunch",
    description: "",
    description_text: null,
    description_html: null,
    network: 2,
    identifier: "WEB",
    default_assignee: null,
    project_lead: null,
    emoji: null,
    icon_prop: null,
    module_view: false,
    cycle_view: false,
    issue_views_view: false,
    page_view: false,
    intake_view: false,
    inbox_view: false,
    is_time_tracking_enabled: false,
    is_issue_type_enabled: false,
    is_default: false,
    guest_view_all_features: false,
    members_can_edit_states: false,
    cover_image: null,
    cover_image_asset: null,
    cover_image_url: null,
    estimate: null,
    archive_in: 0,
    close_in: 0,
    logo_props: {},
    default_state: null,
    archived_at: null,
    timezone: "UTC",
    external_source: null,
    external_id: null,
    repo_url: "",
    base_branch: "",
    is_favorite: false,
    sort_order: null,
    member_role: 15,
    anchor: null,
    members: [],
    next_work_item_sequence: 1,
    agent_executor_options: null,
    ...overrides,
  };
}

function stubClient(
  args: { favoriteStatus?: number; gate?: Promise<void>; calls?: string[]; details?: ProjectDetails[] } = {}
) {
  const calls = args.calls ?? [];
  const favoriteStatus = args.favoriteStatus ?? 204;
  const details = args.details ?? [project({ is_favorite: favoriteStatus === 204 })];
  createAppClient("https://api.test", (async (url: string, init?: { method?: string }) => {
    calls.push(`${init?.method ?? "GET"} ${url}`);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify({ csrf_token: "csrf" }),
      };
    }
    if (url.includes("/user-favorite-projects")) {
      if (args.gate) await args.gate;
      return {
        status: favoriteStatus,
        url,
        headers: { get: () => null },
        text: async () => (favoriteStatus === 204 ? "" : JSON.stringify({ detail: " starring is unavailable " })),
      };
    }
    if (url.endsWith("/projects/details/")) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(details),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  }) as never);
  return calls;
}

function Probe({ row }: { row: ProjectDetails }): React.ReactElement {
  const toggle = useToggleFavoriteProject();
  const [outcome, setOutcome] = React.useState("idle");
  return (
    <>
      <p data-testid="pending">{toggle.pendingProjectId ?? "none"}</p>
      <p data-testid="outcome">{outcome}</p>
      <button type="button" onClick={() => toggle.mutate({ workspaceSlug: "acme", project: row })}>
        toggle star
      </button>
      <button
        type="button"
        onClick={() => {
          void toggle.mutateAsync({ workspaceSlug: "acme", project: row }).then(
            () => setOutcome("ok"),
            () => setOutcome("failed")
          );
        }}
      >
        toggle star async
      </button>
    </>
  );
}

function renderProbe(row: ProjectDetails, seed = true) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (seed) queryClient.setQueryData(projectKeys.details("acme"), [row]);
  render(
    <QueryClientProvider client={queryClient}>
      <Probe row={row} />
    </QueryClientProvider>
  );
  return queryClient;
}

function starred(queryClient: QueryClient): boolean | undefined {
  return queryClient.getQueryData<ProjectDetails[]>(projectKeys.details("acme"))?.[0]?.is_favorite;
}

beforeEach(() => {
  vi.clearAllMocks();
  globalThis.localStorage.clear();
});

afterEach(() => {
  resetAppClient();
});

describe("useToggleFavoriteProject", () => {
  it("stars with an optimistic flip, then opens the favorites section on first add", async () => {
    const calls = stubClient();
    const queryClient = renderProbe(project());

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(true));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(FAVORITE_ADDED_TITLE, expect.any(String)));
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/user-favorite-projects/"))).toBe(true);
    expect(globalThis.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBe("1");
  });

  it("leaves an already-open favorites section alone", async () => {
    globalThis.localStorage.setItem(FAVORITES_SECTION_OPEN_KEY, "1");
    stubClient();
    renderProbe(project());

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(FAVORITE_ADDED_TITLE, expect.any(String)));
    expect(globalThis.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBe("1");
  });

  it("unstars a favorited project", async () => {
    const calls = stubClient();
    const queryClient = renderProbe(project({ is_favorite: true }));

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(false));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(FAVORITE_REMOVED_TITLE, expect.any(String)));
    expect(
      calls.some((call) => call.startsWith("DELETE") && call.includes(`/user-favorite-projects/${PROJECT_ID}/`))
    ).toBe(true);
    expect(globalThis.localStorage.getItem(FAVORITES_SECTION_OPEN_KEY)).toBeNull();
  });

  it("rolls the star back and toasts when the write fails", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubClient({ favoriteStatus: 400, gate });
    const queryClient = renderProbe(project());

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(starred(queryClient)).toBe(true));
    release();
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(FAVORITE_FAILED_TITLE, expect.any(String)));
    await waitFor(() => expect(starred(queryClient)).toBe(false));
    expect(notifySuccess).not.toHaveBeenCalled();
  });

  it("toggles without a seeded collection", async () => {
    const calls = stubClient();
    renderProbe(project(), false);

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(FAVORITE_ADDED_TITLE, expect.any(String)));
    expect(calls.some((call) => call.startsWith("POST") && call.includes("/user-favorite-projects/"))).toBe(true);
  });

  it("pins the pending marker on the toggling card", async () => {
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    stubClient({ gate });
    renderProbe(project());

    expect(screen.getByTestId("pending")).toHaveTextContent("none");
    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(screen.getByTestId("pending")).toHaveTextContent(PROJECT_ID));
    release();
    await waitFor(() => expect(screen.getByTestId("pending")).toHaveTextContent("none"));
  });

  it("leaves sibling rows untouched", async () => {
    const sibling = project({
      id: "323e4567-e89b-12d3-a456-426614174002",
      name: "Sibling",
      is_favorite: true,
    });
    stubClient({ details: [project({ is_favorite: true }), sibling] });
    const queryClient = renderProbe(project());
    queryClient.setQueryData(projectKeys.details("acme"), [project(), sibling]);

    fireEvent.click(screen.getByRole("button", { name: "toggle star" }));
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith(FAVORITE_ADDED_TITLE, expect.any(String)));
    const rows = queryClient.getQueryData<ProjectDetails[]>(projectKeys.details("acme"));
    expect(rows?.find((row) => row.id === PROJECT_ID)?.is_favorite).toBe(true);
    expect(rows?.find((row) => row.id === sibling.id)?.is_favorite).toBe(true);
  });

  it("surfaces a corrupt collection instead of crashing the rollback", async () => {
    stubClient();
    const queryClient = renderProbe(project(), false);
    queryClient.setQueryData(projectKeys.details("acme"), "corrupt" as never);

    fireEvent.click(screen.getByRole("button", { name: "toggle star async" }));
    await waitFor(() => expect(screen.getByTestId("outcome")).toHaveTextContent("failed"));
    expect(notifySuccess).not.toHaveBeenCalled();
  });
});

describe("selectPendingProjectId", () => {
  const variables: ToggleFavoriteInput = { workspaceSlug: "acme", project: project() };

  it("pins null when idle", () => {
    expect(selectPendingProjectId(false, undefined)).toBeNull();
    expect(selectPendingProjectId(false, variables)).toBeNull();
  });

  it("pins the toggling card while flying", () => {
    expect(selectPendingProjectId(true, variables)).toBe(PROJECT_ID);
  });

  it("pins null on a pending mutation without variables", () => {
    expect(selectPendingProjectId(true, undefined)).toBeNull();
  });
});
