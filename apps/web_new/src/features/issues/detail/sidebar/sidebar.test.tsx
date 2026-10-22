// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { Issue, ProjectDetail, TransportInit } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../../core/api/client.js";
import { resetRegisteredStores } from "../../../../core/session/index.js";
import { setSessionClient } from "../../../../core/session/queries.js";
import { agentReasonMessage } from "../../../../shared/lib/agentReasons.js";
import { pickerKeys } from "../../../../shared/pickers/index.js";
import { notifyError } from "../../../../shared/shell/index.js";
import { pickerStatesQueryOptions } from "../../../../shared/pickers/reference.js";
import { issueKeys } from "../../api/keys.js";
import { issueDetailQueryOptions } from "../../api/queries.js";
import { CreatedByRow } from "./CreatedByRow.js";
import { PRODUCT_CREATOR_NAME, UNKNOWN_CREATOR_NAME, isIntakeBot, selectCreator } from "./creator.js";
import { IssuePropertiesSidebar, PROPERTIES_LOAD_FAILED } from "./IssuePropertiesSidebar.js";
import { SidebarProperties, SidebarPropertiesSkeleton, selectExecutionSelection } from "./SidebarProperties.js";
import { SidebarRow } from "./SidebarRow.js";

vi.mock("../../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../../shared/shell/index.js")>();
  return { ...original, notifyError: vi.fn() };
});

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ISSUE = "123e4567-e89b-12d3-a456-426614174000";
const OTHER_ISSUE = "923e4567-e89b-12d3-a456-426614174009";
const ADA = "323e4567-e89b-12d3-a456-426614174002";
const BOT = "423e4567-e89b-12d3-a456-426614174003";
const AGENT_BOT = "433e4567-e89b-12d3-a456-426614174003";
const STARRED = "523e4567-e89b-12d3-a456-426614174004";
const DONE_STATE = "623e4567-e89b-12d3-a456-426614174005";
const STARTED_STATE = "723e4567-e89b-12d3-a456-426614174006";
const POD_A = "823e4567-e89b-12d3-a456-426614174007";
const POD_B = "a23e4567-e89b-12d3-a456-426614174008";
const WORKSPACE_ID = "b23e4567-e89b-12d3-a456-42661417400e";
const RUNNER_GPU = "c23e4567-e89b-12d3-a456-42661417400f";

const ME = "023e4567-e89b-12d3-a456-426614174009";

const ME_PAYLOAD = {
  id: ME,
  email: "ada@example.com",
  username: "ada",
  first_name: "Ada",
  last_name: "L",
  display_name: "Ada L",
  avatar: "",
  avatar_url: null,
  cover_image: null,
  cover_image_url: null,
  date_joined: "2026-01-01T00:00:00.000Z",
  user_timezone: "UTC",
  is_active: true,
  is_bot: false,
  is_email_verified: true,
  is_password_autoset: false,
  last_login_medium: "password",
  last_login_time: null,
};

function issueFixture(overrides: Partial<Issue> = {}): Issue {
  return {
    id: ISSUE,
    name: "Fix login",
    state_id: DONE_STATE,
    sort_order: 1,
    completed_at: null,
    estimate_point: null,
    priority: "high",
    sequence_id: 42,
    project_id: PROJECT,
    parent_id: null,
    cycle_id: null,
    module_ids: [],
    label_ids: [],
    assignee_ids: [],
    sub_issues_count: null,
    attachment_count: null,
    link_count: null,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: ADA,
    updated_by: null,
    is_draft: false,
    archived_at: null,
    complexity_score: 0,
    agent_executor: "local_runner",
    assigned_pod_id: POD_A,
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

function projectFixture(overrides: Partial<ProjectDetail> = {}): ProjectDetail {
  return {
    id: PROJECT,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    created_by: null,
    updated_by: null,
    workspace: ISSUE,
    name: "Web",
    network: 2,
    identifier: "WEB",
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
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "cloud_agent_unavailable" },
      { kind: "local_runner", available: true, reason_code: "" },
    ],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

const STATES = [
  {
    id: DONE_STATE,
    project_id: PROJECT,
    workspace_id: ISSUE,
    name: "Done",
    color: "#00ff00",
    group: "completed",
    default: false,
    description: "",
    sequence: 2,
  },
  {
    id: STARTED_STATE,
    project_id: PROJECT,
    workspace_id: ISSUE,
    name: "In progress",
    color: "#ffaa00",
    group: "started",
    default: false,
    description: "",
    sequence: 1,
  },
];

const DIRECTORY = [
  {
    id: "b23e4567-e89b-12d3-a456-42661417400a",
    member: {
      id: ADA,
      first_name: "Ada",
      last_name: "L",
      avatar: "",
      avatar_url: "https://cdn.test/ada.png",
      display_name: "Ada L",
      is_bot: false,
    },
    role: 20,
    is_active: true,
  },
  {
    id: "c23e4567-e89b-12d3-a456-42661417400b",
    member: {
      id: BOT,
      first_name: "Intake",
      last_name: "",
      avatar: "",
      avatar_url: "https://cdn.test/agent.png",
      display_name: "acme-intake",
      is_bot: true,
    },
    role: 15,
    is_active: true,
  },
  {
    id: "c43e4567-e89b-12d3-a456-42661417400b",
    member: {
      id: AGENT_BOT,
      first_name: "Agent",
      last_name: "Runner",
      avatar: "",
      avatar_url: null,
      display_name: "Agent Runner",
      is_bot: true,
    },
    role: 15,
    is_active: true,
  },
];

const PROJECT_MEMBERS = [
  {
    id: "d23e4567-e89b-12d3-a456-42661417400c",
    role: 20,
    member: ADA,
    project: PROJECT,
    original_role: 20,
    created_at: "2024-01-02T03:04:05Z",
  },
  {
    id: "e23e4567-e89b-12d3-a456-42661417400d",
    role: 20,
    member: ME,
    project: PROJECT,
    original_role: 20,
    created_at: "2024-01-02T03:04:05Z",
  },
];

function podRow(id: string, name: string, isDefault: boolean) {
  return {
    id,
    name,
    description: "",
    is_default: isDefault,
    workspace: ISSUE,
    project: PROJECT,
    project_identifier: "WEB",
    created_by: null,
    runner_count: 1,
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
  };
}

const PODS = [podRow(POD_A, "WEB_pod_a", true), podRow(POD_B, "WEB_pod_b", false)];

const RUNNERS = [
  {
    id: RUNNER_GPU,
    name: "gpu-box",
    status: "online",
    host_label: "ana-desktop",
    provisioning: "manual",
    os: "linux",
    arch: "x86_64",
    runner_version: "1.2.3",
    dev_metadata: {},
    protocol_version: 4,
    capabilities: [],
    last_heartbeat_at: "2024-02-03T04:05:06Z",
    owner: ADA,
    dev_machine: null,
    dev_machine_detail: null,
    visibility: 0,
    pod: POD_A,
    pod_detail: {
      id: POD_A,
      name: "WEB_pod_a",
      is_default: true,
      project: PROJECT,
      project_identifier: "WEB",
    },
    live_state: null,
    enrolled_at: "2024-01-02T03:04:05Z",
    revoked_at: null,
    revoked_reason: "",
    created_at: "2024-01-02T03:04:05Z",
    updated_at: "2024-02-03T04:05:06Z",
    assignable: true,
    not_assignable_code: "",
    is_busy: false,
    queue_depth: 0,
  },
];

const WORKSPACES = [{ id: WORKSPACE_ID, name: "Acme", slug: SLUG, logo_url: null, total_members: 2, role: 20 }];

interface HarnessOptions {
  project?: Partial<ProjectDetail>;
  patchStatus?: number;
  membersStatus?: number;
  projectStatus?: number;
  statesStatus?: number;
  podsStatus?: number;
  runners?: unknown;
  directory?: unknown;
  /** Keeps the issue PATCH open until the returned release runs. */
  holdPatch?: boolean;
  /** Keeps the workspace directory read open until the returned release runs. */
  holdDirectory?: boolean;
}

function harness(options: HarnessOptions = {}) {
  const writes: Array<{ url: string; body: unknown }> = [];
  const reads: string[] = [];
  let releasePatch: (() => void) | null = null;
  const patchGate = options.holdPatch
    ? new Promise<void>((resolve) => {
        releasePatch = resolve;
      })
    : null;
  let releaseDirectory: (() => void) | null = null;
  const directoryGate = options.holdDirectory
    ? new Promise<void>((resolve) => {
        releaseDirectory = resolve;
      })
    : null;
  const conversation = async (url: string, init: TransportInit) => {
    if (init.method === "GET") reads.push(url);
    if (url.includes(`/issues/${OTHER_ISSUE}/`)) throw new Error(`unexpected other issue ${url}`);
    if (init.method === "PATCH" && url.includes(`/issues/${ISSUE}/`)) {
      writes.push({ url, body: JSON.parse(init.body ?? "{}") });
      if (patchGate !== null) await patchGate;
      const status = options.patchStatus ?? 204;
      return {
        status,
        url,
        headers: { get: () => null },
        text: async () => (status === 204 ? "" : JSON.stringify({ error: "nope" })),
      };
    }
    if (init.method === "PATCH") {
      writes.push({ url, body: JSON.parse(init.body ?? "{}") });
      return { status: 200, url, headers: { get: () => null }, text: async () => "{}" };
    }
    if (url.endsWith("/auth/get-csrf-token/")) {
      return { status: 200, url, headers: { get: () => "application/json" }, text: async () => '{"csrf_token":"cs"}' };
    }
    if (url.endsWith("/api/users/me/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(ME_PAYLOAD),
      };
    }
    if (url.endsWith(`/projects/${PROJECT}/`)) {
      const status = options.projectStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => "application/json" },
        text: async () => (status === 200 ? JSON.stringify(projectFixture(options.project)) : "{}"),
      };
    }
    if (url.includes(`/projects/${PROJECT}/members/`)) {
      const status = options.membersStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => "application/json" },
        text: async () => (status === 200 ? JSON.stringify(PROJECT_MEMBERS) : "{}"),
      };
    }
    if (url.includes(`/projects/${PROJECT}/states/`)) {
      const status = options.statesStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => "application/json" },
        text: async () => (status === 200 ? JSON.stringify(STATES) : "{}"),
      };
    }
    if (url.includes("/api/runners/pods/")) {
      const status = options.podsStatus ?? 200;
      return {
        status,
        url,
        headers: { get: () => "application/json" },
        text: async () => (status === 200 ? JSON.stringify(PODS) : "{}"),
      };
    }
    if (new URL(url).pathname === "/api/runners/") {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(options.runners ?? []),
      };
    }
    if (url.endsWith(`/workspaces/${SLUG}/members/`)) {
      if (directoryGate !== null) await directoryGate;
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => JSON.stringify(options.directory ?? DIRECTORY),
      };
    }
    return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
  };
  const client = createAppClient("https://api.test", conversation as never);
  setSessionClient(client);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(["session", "me"], ME_PAYLOAD);
  queryClient.setQueryData(["session", "workspaces"], WORKSPACES);
  return {
    client,
    queryClient,
    writes,
    reads,
    releasePatch: () => releasePatch?.(),
    releaseDirectory: () => releaseDirectory?.(),
  };
}

function wrap(node: React.ReactElement, queryClient: QueryClient) {
  return render(React.createElement(QueryClientProvider, { client: queryClient }, node));
}

function row(name: string): HTMLElement {
  const group = screen.getByRole("group", { name });
  return group;
}

afterEach(() => {
  cleanup();
  resetAppClient();
  resetRegisteredStores();
  vi.clearAllMocks();
});

beforeEach(() => {
  createAppClient("https://api.test");
});

describe("creator display", () => {
  it("names a real member with their avatar", () => {
    expect(selectCreator(DIRECTORY, ADA)).toEqual({
      name: "Ada L",
      avatarUrl: "https://cdn.test/ada.png",
      masked: false,
    });
  });

  it("masks an intake-bot creator under the product name", () => {
    expect(selectCreator(DIRECTORY, BOT)).toEqual({ name: PRODUCT_CREATOR_NAME, avatarUrl: null, masked: true });
  });

  it("keeps a non-intake bot's own name and avatar", () => {
    expect(selectCreator(DIRECTORY, AGENT_BOT)).toEqual({ name: "Agent Runner", avatarUrl: null, masked: false });
  });

  it("recognises the intake bot by its display-name marker only", () => {
    expect(isIntakeBot({ display_name: "acme-intake" })).toBe(true);
    expect(isIntakeBot({ display_name: "Acme-Intake" })).toBe(true);
    expect(isIntakeBot({ display_name: "Intake Bot" })).toBe(false);
    expect(isIntakeBot({ display_name: "Agent Runner" })).toBe(false);
  });

  it("falls back for a creator outside the directory and stays empty without one", () => {
    expect(selectCreator(DIRECTORY, STARRED)).toEqual({ name: UNKNOWN_CREATOR_NAME, avatarUrl: null, masked: false });
    expect(selectCreator(undefined, ADA)).toEqual({ name: UNKNOWN_CREATOR_NAME, avatarUrl: null, masked: false });
    expect(selectCreator(DIRECTORY, null)).toBeNull();
  });

  it("renders nothing without a creator, and initials without an avatar", () => {
    const { container, rerender } = wrap(<CreatedByRow creator={null} />, new QueryClient());
    expect(container).toBeEmptyDOMElement();
    rerender(
      React.createElement(
        QueryClientProvider,
        { client: new QueryClient() },
        <CreatedByRow creator={{ name: "Ada L", avatarUrl: null, masked: false }} />
      )
    );
    expect(within(row("Created by")).getByRole("img", { name: "Ada L" })).toHaveTextContent("AL");
  });

  it("renders a neutral placeholder while pending and the name once settled", () => {
    const creator = { name: "Ada L", avatarUrl: "https://cdn.test/ada.png", masked: false };
    const { rerender } = wrap(<CreatedByRow creator={creator} pending />, new QueryClient());
    expect(row("Created by")).toBeInTheDocument();
    expect(row("Created by")).not.toHaveTextContent("Ada L");
    expect(within(row("Created by")).queryByRole("img")).not.toBeInTheDocument();
    rerender(
      React.createElement(QueryClientProvider, { client: new QueryClient() }, <CreatedByRow creator={creator} />)
    );
    expect(row("Created by")).toHaveTextContent("Ada L");
  });

  it("renders the creator read-only with no control", () => {
    wrap(
      <CreatedByRow creator={{ name: "Ada L", avatarUrl: "https://cdn.test/ada.png", masked: false }} />,
      new QueryClient()
    );
    expect(row("Created by")).toHaveTextContent("Ada L");
    expect(within(row("Created by")).queryByRole("button")).not.toBeInTheDocument();
    expect(within(row("Created by")).getByRole("img", { name: "Ada L" })).toBeInTheDocument();
  });
});

describe("SidebarRow", () => {
  it("labels its control group", () => {
    wrap(
      <SidebarRow label="State">
        <button type="button">Done</button>
      </SidebarRow>,
      new QueryClient()
    );
    expect(within(row("State")).getByRole("button", { name: "Done" })).toBeInTheDocument();
  });
});

describe("selectExecutionSelection", () => {
  const localProject = { default_agent_executor: "local_runner" };

  it("pins cloud and managed agents over the pod", () => {
    expect(selectExecutionSelection(issueFixture({ agent_executor: "cloud_agent" }), localProject)).toEqual({
      type: "agent",
      executor: "cloud_agent",
    });
    expect(selectExecutionSelection(issueFixture({ agent_executor: "managed_runner" }), localProject)).toEqual({
      type: "agent",
      executor: "managed_runner",
    });
  });

  it("reads the pod for the local executor and nothing for an unpinned local item", () => {
    expect(selectExecutionSelection(issueFixture(), localProject)).toEqual({ type: "pod", podId: POD_A });
    expect(selectExecutionSelection(issueFixture({ assigned_pod_id: null }), localProject)).toBeNull();
    expect(selectExecutionSelection(issueFixture({ assigned_pod_id: undefined }), localProject)).toBeNull();
  });

  it("reads the pinned runner over the pod", () => {
    expect(
      selectExecutionSelection(issueFixture({ assigned_pod_id: POD_A, assigned_runner_id: "runner-1" }), localProject)
    ).toEqual({ type: "runner", runnerId: "runner-1", podId: POD_A });
  });

  it("keeps a runner without a pod readable with an empty pod", () => {
    expect(
      selectExecutionSelection(issueFixture({ assigned_pod_id: null, assigned_runner_id: "runner-1" }), localProject)
    ).toEqual({ type: "runner", runnerId: "runner-1", podId: "" });
  });

  it("inherits a cloud project default when the issue pins no executor", () => {
    expect(
      selectExecutionSelection(issueFixture({ agent_executor: null }), { default_agent_executor: "cloud_agent" })
    ).toEqual({ type: "agent", executor: "cloud_agent" });
  });

  it("inherits a managed project default when the issue pins no executor", () => {
    expect(
      selectExecutionSelection(issueFixture({ agent_executor: null, assigned_pod_id: null }), {
        default_agent_executor: "managed_runner",
      })
    ).toEqual({ type: "agent", executor: "managed_runner" });
  });

  it("keeps an explicit issue override over the project default", () => {
    expect(
      selectExecutionSelection(issueFixture({ agent_executor: "local_runner", assigned_pod_id: POD_B }), {
        default_agent_executor: "cloud_agent",
      })
    ).toEqual({ type: "pod", podId: POD_B });
  });
});

/**
 * The rows as the detail route renders them: the issue comes from the
 * detail cache through the same query the page uses, so an optimistic
 * paint is visible exactly as a reader would see it while the PATCH is
 * still in flight.
 */
function CachedSidebar({
  client,
  editable = true,
  project = projectFixture(),
}: {
  client: ReturnType<typeof createAppClient>;
  editable?: boolean;
  project?: ProjectDetail;
}): React.ReactElement {
  const issue = useQuery({ ...issueDetailQueryOptions(client, SLUG, PROJECT, ISSUE), enabled: true });
  if (issue.data === undefined) return <p>loading issue</p>;
  return (
    <SidebarProperties
      workspaceSlug={SLUG}
      projectId={PROJECT}
      issue={issue.data}
      project={project}
      editable={editable}
    />
  );
}

function seedDetail(queryClient: QueryClient, overrides: Partial<Issue> = {}): void {
  queryClient.setQueryData(issueKeys.detail(SLUG, ISSUE), issueFixture(overrides));
}

describe("SidebarProperties immediate updates", () => {
  it("moves the state row before the PATCH settles", async () => {
    const { client, queryClient, releasePatch, writes } = harness({ holdPatch: true });
    seedDetail(queryClient);
    wrap(<CachedSidebar client={client} />, queryClient);
    // States load lazily on open, so the trigger reads the picked name
    // once the list has answered; the row itself reads the issue cache.
    fireEvent.click(within(row("State")).getByRole("button"));
    await waitFor(() => expect(within(row("State")).getByText("Done")).toBeInTheDocument());
    fireEvent.click(await screen.findByRole("option", { name: "In progress" }));
    await waitFor(() => expect(within(row("State")).getByText("In progress")).toBeInTheDocument());
    expect(queryClient.getQueryData<Issue>(issueKeys.detail(SLUG, ISSUE))?.state_id).toBe(STARTED_STATE);
    expect(writes[0]?.body).toEqual({ state_id: STARTED_STATE });
    releasePatch();
    await waitFor(() => expect(within(row("State")).getByText("In progress")).toBeInTheDocument());
  });

  it("names the assignee before the PATCH settles", async () => {
    const { client, queryClient, releasePatch, writes } = harness({ holdPatch: true });
    seedDetail(queryClient);
    wrap(<CachedSidebar client={client} />, queryClient);
    expect(within(row("Assignees")).getByRole("button")).toHaveTextContent("Add assignees");
    fireEvent.click(within(row("Assignees")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Ada L" }));
    await waitFor(() =>
      expect(queryClient.getQueryData<Issue>(issueKeys.detail(SLUG, ISSUE))?.assignee_ids).toEqual([ADA])
    );
    expect(row("Assignees")).toHaveTextContent("Ada L");
    expect(writes).toHaveLength(1);
    releasePatch();
  });

  it("shows the picked priority before the PATCH settles", async () => {
    const { client, queryClient, releasePatch } = harness({ holdPatch: true });
    seedDetail(queryClient);
    wrap(<CachedSidebar client={client} />, queryClient);
    expect(within(row("Priority")).getByRole("button")).toHaveTextContent("High");
    fireEvent.click(within(row("Priority")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Low" }));
    await waitFor(() => expect(within(row("Priority")).getByRole("button")).toHaveTextContent("Low"));
    releasePatch();
  });

  it("rolls all three rows back when the PATCH fails", async () => {
    for (const [label, option, restore] of [
      ["State", "In progress", "Done"],
      ["Priority", "Low", "High"],
    ] as const) {
      const { client, queryClient } = harness({ patchStatus: 500 });
      seedDetail(queryClient);
      const { unmount } = wrap(<CachedSidebar client={client} />, queryClient);
      fireEvent.click(within(row(label)).getByRole("button"));
      fireEvent.click(await screen.findByRole("option", { name: option }));
      await waitFor(() => expect(notifyError).toHaveBeenCalled());
      expect(within(row(label)).getByRole("button")).toHaveTextContent(restore);
      unmount();
      vi.clearAllMocks();
    }
  });

  it("rolls the assignee row back when the PATCH fails", async () => {
    const { client, queryClient } = harness({ patchStatus: 500 });
    seedDetail(queryClient);
    wrap(<CachedSidebar client={client} />, queryClient);
    fireEvent.click(within(row("Assignees")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Ada L" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    await waitFor(() => expect(within(row("Assignees")).getByText("Add assignees")).toBeInTheDocument());
  });
});

describe("SidebarProperties", () => {
  it("renders the property rows in order with a read-only creator", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    const labels = screen.getAllByRole("group").map((group) => group.getAttribute("aria-labelledby") ?? "");
    expect(labels.length).toBe(9);
    for (const text of [
      "State",
      "Assignees",
      "Runs on",
      "Priority",
      "Created by",
      "Start date",
      "Due date",
      "Parent",
      "Labels",
    ]) {
      expect(screen.getByText(text)).toBeInTheDocument();
    }
    await waitFor(() => expect(row("Created by")).toHaveTextContent("Ada L"));
    expect(within(row("Created by")).queryByRole("button")).not.toBeInTheDocument();
  });

  it("skips the member directory when nobody has to be named", async () => {
    const { queryClient, reads } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ created_by: null, assignee_ids: [] })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(reads.some((url) => url.endsWith(`/workspaces/${SLUG}/members/`))).toBe(false);
  });

  it("hides the created-by row when the item records no creator", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ created_by: null })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    expect(screen.queryByText("Created by")).not.toBeInTheDocument();
    expect(screen.getAllByRole("group")).toHaveLength(8);
  });

  it("masks an intake-bot creator in the row", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ created_by: BOT })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    await waitFor(() => expect(row("Created by")).toHaveTextContent(PRODUCT_CREATOR_NAME));
    expect(row("Created by")).not.toHaveTextContent("acme-intake");
  });

  it("names a non-intake bot creator as itself", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ created_by: AGENT_BOT })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    await waitFor(() => expect(row("Created by")).toHaveTextContent("Agent Runner"));
    expect(row("Created by")).not.toHaveTextContent(PRODUCT_CREATOR_NAME);
  });

  it("holds a placeholder while the creator directory is pending", async () => {
    const { queryClient, releaseDirectory } = harness({ holdDirectory: true });
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ created_by: ADA })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    await waitFor(() => expect(row("Created by")).toBeInTheDocument());
    expect(row("Created by")).not.toHaveTextContent(UNKNOWN_CREATOR_NAME);
    expect(row("Created by")).not.toHaveTextContent("Ada L");
    expect(within(row("Created by")).queryByRole("img")).not.toBeInTheDocument();
    await act(async () => {
      releaseDirectory();
    });
    await waitFor(() => expect(row("Created by")).toHaveTextContent("Ada L"));
  });

  it("labels the execution row Pod when the project offers no cloud agent", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture({
          agent_executor_options: [{ kind: "local_runner", available: true, reason_code: "" }],
        })}
        editable
      />,
      queryClient
    );
    expect(screen.getByText("Pod")).toBeInTheDocument();
    expect(screen.queryByText("Runs on")).not.toBeInTheDocument();
  });

  it("renders every control disabled for guests, archived and synced items", () => {
    for (const state of [
      { editable: false },
      { editable: true, issue: { archived_at: "2024-03-04" } },
      { editable: true, issue: { is_synced: true } },
    ]) {
      const { queryClient } = harness();
      const { unmount } = wrap(
        <SidebarProperties
          workspaceSlug={SLUG}
          projectId={PROJECT}
          issue={issueFixture(state.issue)}
          project={projectFixture()}
          editable={state.editable}
        />,
        queryClient
      );
      for (const label of ["State", "Assignees", "Runs on", "Priority"]) {
        const control = within(row(label)).getByRole("button");
        expect(control).toBeDisabled();
      }
      unmount();
    }
  });

  it("names the set values on disabled rows", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ assignee_ids: [ADA] })}
        project={projectFixture()}
        editable={false}
      />,
      queryClient
    );
    await waitFor(() => expect(within(row("State")).getByRole("button")).toHaveTextContent("Done"));
    await waitFor(() => expect(within(row("Assignees")).getByRole("button")).toHaveTextContent("Ada L"));
    await waitFor(() => expect(within(row("Runs on")).getByRole("button")).toHaveTextContent("WEB_pod_a"));
    for (const label of ["State", "Assignees", "Runs on", "Priority"]) {
      expect(within(row(label)).getByRole("button")).toBeDisabled();
    }
  });

  it("names the pinned runner on the Runs on row", async () => {
    const { queryClient } = harness({ runners: RUNNERS });
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ assigned_pod_id: POD_A, assigned_runner_id: RUNNER_GPU })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    await waitFor(() => expect(within(row("Runs on")).getByRole("button")).toHaveTextContent("gpu-box"));
    expect(within(row("Runs on")).getByRole("button")).toHaveTextContent("WEB_pod_a");
  });

  it("badges the Runs on trigger while the run waits", async () => {
    const activeRun = {
      id: "a23e4567-e89b-12d3-a456-42661417400a",
      status: "queued",
      executor_kind: "local_runner",
      queue_position: null,
      runner: null,
      runner_name: null,
      pinned_runner: RUNNER_GPU,
      pinned_runner_name: "gpu-box",
      pin_kind: "dedicated",
      dispatch_wait: {
        reason: "runner_offline",
        pin_kind: "dedicated",
        runner: RUNNER_GPU,
        runner_name: "gpu-box",
        queued_at: "2024-02-03T04:05:06Z",
        runner_last_seen_at: null,
        runs_ahead: 0,
        releasable: true,
      },
      created_at: "2024-02-03T04:05:06Z",
      assigned_at: null,
      started_at: null,
      ended_at: null,
      done_payload: null,
      error: "",
      error_code: "",
      error_diagnostic: null,
      llm_model: "",
      input_tokens: null,
      output_tokens: null,
      total_tokens: null,
      live_state: null,
    };
    const { queryClient } = harness({ runners: RUNNERS });
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({
          assigned_pod_id: POD_A,
          assigned_runner_id: RUNNER_GPU,
          agent_status: { ticker: null, active_run: activeRun, latest_run: activeRun, run_count: 1 },
        })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    await waitFor(() => expect(within(row("Runs on")).getByRole("button")).toHaveTextContent("Offline"));
  });

  it("patches the runner pin from the Runs on picker", async () => {
    const { queryClient, writes } = harness({ runners: RUNNERS });
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    fireEvent.click(within(row("Runs on")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: /gpu-box/ }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.url).toContain(`/issues/${ISSUE}/`);
    expect(writes[0]!.body).toEqual({
      assigned_pod_id: POD_A,
      assigned_runner_id: RUNNER_GPU,
      agent_executor: "local_runner",
    });
  });

  it("patches state with the picked id", async () => {
    const { queryClient, writes } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    fireEvent.click(within(row("State")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "In progress" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.url).toContain(`/issues/${ISSUE}/`);
    expect(writes[0]!.body).toEqual({ state_id: STARTED_STATE });
  });

  it("names a single assignee before the picker opens", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ assignee_ids: [ADA] })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    // The multi picker keeps its own avatar stack (ISS-208); one assignee
    // still reads as their name, not a count.
    await waitFor(() => expect(within(row("Assignees")).getByRole("button")).toHaveTextContent("Ada L"));
    expect(within(row("Assignees")).queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("patches the assignee ids from the multi-select", async () => {
    const { queryClient, writes } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ assignee_ids: [] })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    expect(within(row("Assignees")).getByRole("button")).toHaveTextContent("Add assignees");
    fireEvent.click(within(row("Assignees")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Ada L" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ assignee_ids: [ADA] });
  });

  it("patches the priority level", async () => {
    const { queryClient, writes } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    expect(within(row("Priority")).getByRole("button")).toHaveTextContent("High");
    fireEvent.click(within(row("Priority")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: "Low" }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({ priority: "low" });
  });

  it("shows the inherited cloud target for an issue with no executor override", async () => {
    const project = projectFixture({ default_agent_executor: "cloud_agent" });
    const { queryClient } = harness({ project });
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ agent_executor: null })}
        project={project}
        editable
      />,
      queryClient
    );
    // The trigger names the inherited agent, not the pod the issue
    // happens to carry.
    await waitFor(() => expect(within(row("Runs on")).getByRole("button")).toHaveTextContent("Pi Dash Cloud Agent"));
    expect(within(row("Runs on")).queryByText("WEB_pod_a")).not.toBeInTheDocument();
  });

  it("shows the inherited managed target for an issue with no executor override", async () => {
    const project = projectFixture({
      agent_executor_options: [{ kind: "managed_runner", available: true, reason_code: "" }],
      default_agent_executor: "managed_runner",
    });
    const { queryClient } = harness({ project });
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ agent_executor: null, assigned_pod_id: null })}
        project={project}
        editable
      />,
      queryClient
    );
    await waitFor(() => expect(within(row("Pod")).getByRole("button")).toHaveTextContent("Pi Dash Agent (desktop)"));
  });

  it("names the current state before the picker opens", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    await waitFor(() => expect(within(row("State")).getByRole("button")).toHaveTextContent("Done"));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("prompts for an issue with no state, assignees or target", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture({ state_id: null, assignee_ids: [], assigned_pod_id: null })}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    expect(within(row("State")).getByRole("button")).toHaveTextContent("Set state");
    expect(within(row("Assignees")).getByRole("button")).toHaveTextContent("Add assignees");
    expect(within(row("Runs on")).getByRole("button")).toHaveTextContent("Select target");
    await waitFor(() => expect(queryClient.getQueryData(pickerKeys.states(SLUG, PROJECT))).toBeDefined());
    // The states load with the panel because the overdue highlight reads
    // the state group; the pod list still waits for its picker.
    expect(queryClient.getQueryData(pickerKeys.pods(SLUG, PROJECT))).toBeUndefined();
  });

  it("names a pinned pod before the picker opens", async () => {
    const { queryClient } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    await waitFor(() => expect(within(row("Runs on")).getByRole("button")).toHaveTextContent("WEB_pod_a"));
    expect(within(row("Runs on")).getByRole("button").querySelector("svg")).not.toBeNull();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("patches a pod pick to the pod plus the local executor", async () => {
    const { queryClient, writes } = harness();
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    fireEvent.click(within(row("Runs on")).getByRole("button"));
    await screen.findByRole("option", { name: /WEB_pod_b/ });
    // Unavailable agent rows carry their reason and never activate.
    const cloud = screen.getByRole("option", { name: /Pi Dash Cloud Agent/ });
    expect(cloud).toBeDisabled();
    expect(cloud).toHaveTextContent(agentReasonMessage("cloud_agent_unavailable"));
    fireEvent.click(screen.getByRole("option", { name: /WEB_pod_b/ }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({
      assigned_pod_id: POD_B,
      assigned_runner_id: null,
      agent_executor: "local_runner",
    });
  });

  it("patches a managed-agent pick to the executor with no pod", async () => {
    const project = projectFixture({
      agent_executor_options: [{ kind: "managed_runner", available: true, reason_code: "" }],
    });
    const { queryClient, writes } = harness({ project });
    wrap(
      <SidebarProperties workspaceSlug={SLUG} projectId={PROJECT} issue={issueFixture()} project={project} editable />,
      queryClient
    );
    fireEvent.click(within(row("Pod")).getByRole("button"));
    fireEvent.click(await screen.findByRole("option", { name: /Pi Dash Agent \(desktop\)/ }));
    await waitFor(() => expect(writes).toHaveLength(1));
    expect(writes[0]!.body).toEqual({
      assigned_pod_id: null,
      assigned_runner_id: null,
      agent_executor: "managed_runner",
    });
  });

  it("reports a refused write and keeps the row on the server value", async () => {
    const { queryClient } = harness({ patchStatus: 500 });
    queryClient.setQueryData(issueKeys.detail(SLUG, ISSUE), issueFixture());
    wrap(
      <SidebarProperties
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        project={projectFixture()}
        editable
      />,
      queryClient
    );
    fireEvent.click(within(row("State")).getByRole("button"));
    const option = await screen.findByRole("option", { name: "In progress" });
    fireEvent.click(option);
    await waitFor(() => expect(notifyError).toHaveBeenCalled());
    expect(queryClient.getQueryData<Issue>(issueKeys.detail(SLUG, ISSUE))?.state_id).toBe(DONE_STATE);
    expect(within(row("State")).getByRole("button")).toHaveTextContent("Done");
  });
});

describe("IssuePropertiesSidebar", () => {
  it("shows a skeleton until the project row hydrates", async () => {
    const { queryClient } = harness();
    let release: (() => void) | null = null;
    let gatedOnce = false;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const gated = createAppClient("https://api.test", async (url: string, init: TransportInit) => {
      if (!gatedOnce && init.method === "GET" && url.endsWith(`/projects/${PROJECT}/`)) {
        gatedOnce = true;
        await gate;
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => JSON.stringify(projectFixture()),
        };
      }
      if (url.endsWith(`/workspaces/${SLUG}/members/`)) {
        return {
          status: 200,
          url,
          headers: { get: () => "application/json" },
          text: async () => JSON.stringify(DIRECTORY),
        };
      }
      return { status: 404, url, headers: { get: () => null }, text: async () => "{}" };
    });
    wrap(
      <IssuePropertiesSidebar
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        editable
        client={gated}
      />,
      queryClient
    );
    expect(screen.getByRole("status", { name: "Loading properties" })).toBeInTheDocument();
    await act(async () => {
      release?.();
    });
    expect(await screen.findByRole("group", { name: "Runs on" })).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: "Loading properties" })).not.toBeInTheDocument();
  });

  it("renders the rows once the project row lands", async () => {
    const { client, queryClient } = harness();
    wrap(
      <IssuePropertiesSidebar
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        editable
        client={client}
      />,
      queryClient
    );
    expect(await screen.findByRole("group", { name: "Runs on" })).toBeInTheDocument();
    await waitFor(() => expect(row("Created by")).toHaveTextContent("Ada L"));
  });

  it("offers a retry when the project row fails", async () => {
    const { client, queryClient } = harness({ projectStatus: 500 });
    wrap(
      <IssuePropertiesSidebar
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        editable
        client={client}
      />,
      queryClient
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(PROPERTIES_LOAD_FAILED);
    const retry = screen.getByRole("button", { name: "Try again" });
    fireEvent.click(retry);
    // The retry runs the same read again; the test transport keeps
    // answering 500, so the alert stays.
    expect(await screen.findByRole("alert")).toHaveTextContent(PROPERTIES_LOAD_FAILED);
  });

  it("renders the skeleton as a status region with five rows", () => {
    wrap(<SidebarPropertiesSkeleton />, new QueryClient());
    expect(screen.getAllByRole("status", { name: "Loading properties" })).toHaveLength(1);
    expect(screen.getByRole("status", { name: "Loading properties" }).children).toHaveLength(5);
  });

  it("reads the picker caches the rows share", async () => {
    const { client, queryClient } = harness();
    void pickerKeys;
    void pickerStatesQueryOptions;
    wrap(
      <IssuePropertiesSidebar
        workspaceSlug={SLUG}
        projectId={PROJECT}
        issue={issueFixture()}
        editable
        client={client}
      />,
      queryClient
    );
    await screen.findByRole("group", { name: "Runs on" });
    await waitFor(() => expect(queryClient.getQueryData(pickerKeys.project(SLUG, PROJECT))).toBeDefined());
    await waitFor(() => expect(queryClient.getQueryData(pickerKeys.workspaceMembers(SLUG))).toBeDefined());
  });
});
