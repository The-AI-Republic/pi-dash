// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { Issue, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { setSessionClient } from "../../../core/session/queries.js";
import { notifyError } from "../../../shared/shell/index.js";
import { issueKeys } from "../api/keys.js";
import {
  PARENT_BANNER_LABEL,
  ParentBanner,
  REMOVE_PARENT_TEXT,
  SIBLINGS_EMPTY_TEXT,
  SIBLINGS_LOADING_TEXT,
} from "./ParentBanner.js";

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_PROJECT = "323e4567-e89b-12d3-a456-426614174002";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const PARENT = "423e4567-e89b-12d3-a456-426614174003";
const SIBLING = "523e4567-e89b-12d3-a456-426614174004";
const STATE = "623e4567-e89b-12d3-a456-426614174005";
const OTHER_STATE = "723e4567-e89b-12d3-a456-426614174006";

function issue(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Child item",
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    sequence_id: 7,
    project_id: PROJECT,
    parent_id: PARENT,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: null,
    is_synced: false,
    description_html: "<p>words</p>",
    is_subscribed: false,
    agent_ticker: null,
    agent_status: null,
    relations_summary: { blocked_by: [], blocking: [] },
    has_open_blockers: false,
    ...overrides,
  };
}

function project(id: string, identifier: string) {
  return {
    id,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    workspace: ISSUE,
    name: identifier,
    network: 2,
    identifier,
    sort_order: 1,
    logo_props: {},
    member_role: 20,
    archived_at: null,
    cycle_view: false,
    issue_views_view: true,
    module_view: false,
    page_view: true,
    inbox_view: true,
    is_default: true,
    guest_view_all_features: false,
    project_lead: null,
    estimate: null,
    agent_executor_options: [],
    default_agent_executor: "local_runner",
  };
}

const STATES = [
  {
    id: STATE,
    project_id: PROJECT,
    workspace_id: ISSUE,
    name: "Started",
    color: "#ffaa00",
    group: "started",
    default: false,
    description: "",
    sequence: 1,
  },
  {
    id: OTHER_STATE,
    project_id: PROJECT,
    workspace_id: ISSUE,
    name: "Done",
    color: "#00ff00",
    group: "completed",
    default: false,
    description: "",
    sequence: 2,
  },
];

interface SubIssue {
  id: string;
  name: string;
  sequence_id: number;
  state_group: string | null;
}

function subIssueRow(row: SubIssue): Record<string, unknown> {
  return {
    ...row,
    state_id: STATE,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "none",
    start_date: null,
    target_date: null,
    project_id: PROJECT,
    parent_id: PARENT,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    is_draft: false,
    archived_at: null,
  };
}

interface Options {
  parentProject?: string;
  parentName?: string;
  patchStatus?: number;
  holdPatch?: boolean;
  siblings?: SubIssue[];
  siblingsStatus?: number;
  holdSiblings?: boolean;
  projectStatus?: number;
  holdProject?: boolean;
}

function harness(options: Options = {}) {
  resetAppClient();
  const writes: Array<{ url: string; body: unknown }> = [];
  const reads: string[] = [];
  const parentProject = options.parentProject ?? PROJECT;
  let releasePatch: (() => void) | null = null;
  const gate = options.holdPatch
    ? new Promise<void>((resolve) => {
        releasePatch = resolve;
      })
    : null;
  let releaseSiblings: (() => void) | null = null;
  let releaseProject: (() => void) | null = null;
  const projectGate = options.holdProject
    ? new Promise<void>((resolve) => {
        releaseProject = resolve;
      })
    : null;
  const siblingsGate = options.holdSiblings
    ? new Promise<void>((resolve) => {
        releaseSiblings = resolve;
      })
    : null;
  const conversation = async (url: string, init: TransportInit) => {
    if (init.method === "GET") reads.push(url);
    if (init.method === "PATCH" && url.includes(`/issues/${ISSUE}/`)) {
      writes.push({ url, body: JSON.parse(init.body ?? "{}") });
      if (gate !== null) await gate;
      const status = options.patchStatus ?? 204;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 204 ? "" : '{"error":"nope"}'),
      };
    }
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => null }, text: async () => '{"csrf_token":"c"}' };
    }
    if (url.endsWith(`/issues/${PARENT}/`)) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () =>
          JSON.stringify(
            issue({
              id: PARENT,
              name: options.parentName ?? "Epic parent",
              parent_id: null,
              project_id: parentProject,
              sequence_id: 12,
              state_id: options.parentProject === undefined ? STATE : OTHER_STATE,
            })
          ),
      };
    }
    if (url.includes(`/issues/${PARENT}/sub-issues/`)) {
      if (siblingsGate !== null) await siblingsGate;
      const status = options.siblingsStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () =>
          status === 200
            ? JSON.stringify({
                sub_issues: (
                  options.siblings ?? [
                    { id: ISSUE, name: "Child item", sequence_id: 7, state_group: "started" },
                    { id: SIBLING, name: "Sibling item", sequence_id: 8, state_group: "started" },
                  ]
                ).map(subIssueRow),
                state_distribution: {},
              })
            : "{}",
      };
    }
    if (url.endsWith(`/projects/${PROJECT}/`)) {
      if (projectGate !== null) await projectGate;
      if (projectGate !== null) await projectGate;
      const status = options.projectStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 200 ? JSON.stringify(project(PROJECT, "WEB")) : "{}"),
      };
    }
    if (url.includes(`/projects/${OTHER_PROJECT}/states/`)) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify([]) };
    }
    if (url.endsWith(`/projects/${OTHER_PROJECT}/`)) {
      return {
        status: 200,
        url,
        headers: { get: () => null },
        text: async () => JSON.stringify(project(OTHER_PROJECT, "OPS")),
      };
    }
    if (url.includes(`/projects/${PROJECT}/states/`)) {
      return { status: 200, url, headers: { get: () => null }, text: async () => JSON.stringify(STATES) };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  };
  const client = createAppClient("https://api.test", conversation as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return {
    client,
    queryClient,
    writes,
    reads,
    releasePatch: () => releasePatch?.(),
    releaseSiblings: () => releaseSiblings?.(),
    releaseProject: () => releaseProject?.(),
  };
}

function renderBanner(
  client: ReturnType<typeof harness>["client"],
  queryClient: QueryClient,
  props: Partial<React.ComponentProps<typeof ParentBanner>> = {}
) {
  setSessionClient(client);
  return render(
    <QueryClientProvider client={queryClient}>
      <ParentBanner workspaceSlug={SLUG} projectId={PROJECT} issue={issue()} editable {...props} />
    </QueryClientProvider>
  );
}

afterEach(() => {
  cleanup();
  resetAppClient();
  vi.clearAllMocks();
});

describe("ParentBanner", () => {
  it("renders nothing without a parent", () => {
    const { client, queryClient } = harness();
    renderBanner(client, queryClient, { issue: issue({ parent_id: null }) });
    expect(screen.queryByTestId("parent-banner")).not.toBeInTheDocument();
  });

  it("names the parent's state dot, identifier and name and links to it", async () => {
    const { client, queryClient } = harness();
    renderBanner(client, queryClient);
    const banner = await screen.findByTestId("parent-banner");
    expect(banner).toHaveAttribute("aria-label", PARENT_BANNER_LABEL);
    const link = await within(banner).findByRole("link");
    await waitFor(() => expect(link).toHaveTextContent("WEB-12"));
    expect(link).toHaveAttribute("href", `/acme/browse/WEB-12`);
    expect(link).toHaveTextContent("Epic parent");
    const dot = link.querySelector("span[aria-hidden='true']") as HTMLElement;
    expect(dot.style.backgroundColor).toBe("rgb(255, 170, 0)");
    expect(dot).toHaveAttribute("title", "Started");
  });

  it("uses the parent's own project identifier", async () => {
    const { client, queryClient } = harness({ parentProject: OTHER_PROJECT });
    renderBanner(client, queryClient);
    const link = await within(await screen.findByTestId("parent-banner")).findByRole("link");
    await waitFor(() => expect(link).toHaveTextContent("OPS-12"));
    expect(link).toHaveAttribute("href", `/acme/browse/OPS-12`);
  });

  it("loads siblings lazily, excludes itself and links each one", async () => {
    const { client, queryClient, reads } = harness();
    renderBanner(client, queryClient);
    await within(await screen.findByTestId("parent-banner")).findByRole("link");
    expect(reads.some((url) => url.includes("/sub-issues/"))).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "Parent options" }));
    await new Promise((resolve) => setTimeout(resolve, 200));
    const item = await screen.findByRole("menuitem", { name: /Sibling item/ });
    expect(item).toHaveAttribute("href", `/acme/browse/WEB-8`);
    expect(screen.queryByRole("menuitem", { name: /Child item/ })).not.toBeInTheDocument();
    expect(reads.some((url) => url.includes("/sub-issues/"))).toBe(true);
  });

  it("holds the sibling links until the parent's project identifier is known", async () => {
    const { client, queryClient, releaseProject } = harness({ holdProject: true });
    renderBanner(client, queryClient);
    const banner = await screen.findByTestId("parent-banner");
    fireEvent.click(within(banner).getByRole("button", { name: "Parent options" }));
    expect(await screen.findByText(SIBLINGS_LOADING_TEXT)).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: /Sibling item/ })).not.toBeInTheDocument();
    releaseProject();
    const link = await within(banner).findByRole("link");
    await waitFor(() => expect(link).toHaveTextContent("WEB-12"));
    expect(await screen.findByRole("menuitem", { name: /Sibling item/ })).toHaveAttribute("href", `/acme/browse/WEB-8`);
  });

  it("closes the sibling menu after following a sibling link", async () => {
    const { client, queryClient } = harness();
    renderBanner(client, queryClient);
    await within(await screen.findByTestId("parent-banner")).findByRole("link");
    fireEvent.click(screen.getByRole("button", { name: "Parent options" }));
    const item = await screen.findByRole("menuitem", { name: /Sibling item/ });
    fireEvent.click(item);
    await waitFor(() => expect(screen.queryByRole("menuitem", { name: /Sibling item/ })).not.toBeInTheDocument());
  });

  it("holds the menu with a loading note while the siblings read is pending", async () => {
    const { client, queryClient, releaseSiblings } = harness({ holdSiblings: true });
    renderBanner(client, queryClient);
    await within(await screen.findByTestId("parent-banner")).findByRole("link");
    fireEvent.click(screen.getByRole("button", { name: "Parent options" }));
    expect(await screen.findByText(SIBLINGS_LOADING_TEXT)).toBeInTheDocument();
    releaseSiblings();
    expect(await screen.findByRole("menuitem", { name: /Sibling item/ })).toBeInTheDocument();
    expect(screen.queryByText(SIBLINGS_LOADING_TEXT)).not.toBeInTheDocument();
  });

  it("shows the empty note when the parent has no other children", async () => {
    const { client, queryClient } = harness({
      siblings: [{ id: ISSUE, name: "Child item", sequence_id: 7, state_group: "started" }],
    });
    renderBanner(client, queryClient);
    await within(await screen.findByTestId("parent-banner")).findByRole("link");
    fireEvent.click(screen.getByRole("button", { name: "Parent options" }));
    expect(await screen.findByText(SIBLINGS_EMPTY_TEXT)).toBeInTheDocument();
  });

  it("removes the parent through the issue patch and hides the banner", async () => {
    const { client, queryClient, writes } = harness();
    queryClient.setQueryData(issueKeys.detail(SLUG, ISSUE), issue());
    renderBanner(client, queryClient);
    await within(await screen.findByTestId("parent-banner")).findByRole("link");
    fireEvent.click(screen.getByRole("button", { name: "Parent options" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: REMOVE_PARENT_TEXT }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ parent_id: null });
    expect(queryClient.getQueryData(issueKeys.detail(SLUG, ISSUE))).toMatchObject({ parent_id: null });
    // The read-only callers render from their own issue prop; the row's
    // cached detail is the banner's source, and it is repainted at once.
  });

  it("keeps the banner read-only for guests and archived items", async () => {
    const { client, queryClient } = harness();
    const view = renderBanner(client, queryClient, { editable: false });
    const banner = await screen.findByTestId("parent-banner");
    await within(banner).findByRole("link");
    fireEvent.click(within(banner).getByRole("button", { name: "Parent options" }));
    await screen.findByRole("menuitem", { name: /Sibling item/ });
    expect(screen.queryByRole("menuitem", { name: REMOVE_PARENT_TEXT })).not.toBeInTheDocument();
    view.unmount();
    const second = harness();
    renderBanner(second.client, second.queryClient, { issue: issue({ archived_at: "2026-01-01" }) });
    await within(await screen.findByTestId("parent-banner")).findByRole("link");
    fireEvent.click(screen.getByRole("button", { name: "Parent options" }));
    await screen.findByRole("menuitem", { name: /Sibling item/ });
    expect(screen.queryByRole("menuitem", { name: REMOVE_PARENT_TEXT })).not.toBeInTheDocument();
  });

  it("keeps the banner when the removal is refused", async () => {
    const { client, queryClient } = harness({ patchStatus: 500 });
    renderBanner(client, queryClient);
    await within(await screen.findByTestId("parent-banner")).findByRole("link");
    fireEvent.click(screen.getByRole("button", { name: "Parent options" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: REMOVE_PARENT_TEXT }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(screen.getByTestId("parent-banner")).toBeInTheDocument();
  });

  it("renders the loading copy constant for the sibling read", () => {
    expect(SIBLINGS_LOADING_TEXT).toBe("Loading sibling work items");
  });
});
