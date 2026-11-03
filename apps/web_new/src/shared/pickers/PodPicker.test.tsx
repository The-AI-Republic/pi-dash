// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { setSessionClient } from "../../core/session/queries.js";
import { PodPicker } from "./PodPicker.js";
import type { ExecutionSelection } from "./pods.js";

const PROJECT = "22222222-2222-4222-8222-222222222222";
const WORKSPACE = "33333333-3333-4333-8333-333333333333";
const POD_DEFAULT = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const POD_TIER = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const RUNNER_GPU = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const RUNNER_MAC = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const OWNER_ANA = "44444444-4444-4444-8444-444444444444";
const OWNER_ZOE = "55555555-5555-4555-8555-555555555555";

const WORKSPACES = [{ id: WORKSPACE, name: "Acme", slug: "acme", logo_url: null, total_members: 2, role: 15 }];

const PODS = [
  {
    id: POD_DEFAULT,
    name: "ACME-Backend",
    description: "",
    is_default: true,
    workspace: WORKSPACE,
    project: PROJECT,
    project_identifier: "ACME",
    created_by: null,
    runner_count: 1,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
  },
  {
    id: POD_TIER,
    name: "ACME-Tier",
    description: "",
    is_default: false,
    workspace: WORKSPACE,
    project: PROJECT,
    project_identifier: "ACME",
    created_by: null,
    runner_count: 0,
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
  },
];

function projectRow(overrides: Record<string, unknown> = {}) {
  return {
    id: PROJECT,
    name: "Acme",
    identifier: "ACME",
    sort_order: 1,
    logo_props: {},
    member_role: 1,
    archived_at: null,
    workspace: WORKSPACE,
    cycle_view: true,
    issue_views_view: true,
    module_view: false,
    page_view: false,
    inbox_view: true,
    is_default: false,
    guest_view_all_features: false,
    project_lead: null,
    network: 1,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-02T00:00:00Z",
    created_by: null,
    updated_by: null,
    agent_executor_options: [
      { kind: "cloud_agent", available: false, reason_code: "llm_config_missing" },
      { kind: "local_runner", available: true, reason_code: "" },
      { kind: "managed_runner", available: false, reason_code: "desktop_not_connected" },
    ],
    default_agent_executor: "local_runner",
    ...overrides,
  };
}

function runnerRow(overrides: Record<string, unknown> = {}) {
  return {
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
    last_heartbeat_at: "2026-10-22T03:59:00Z",
    owner: OWNER_ANA,
    dev_machine: null,
    dev_machine_detail: null,
    visibility: 0,
    pod: POD_DEFAULT,
    pod_detail: {
      id: POD_DEFAULT,
      name: "ACME-Backend",
      is_default: true,
      project: PROJECT,
      project_identifier: "ACME",
    },
    live_state: null,
    enrolled_at: "2026-01-02T03:04:05Z",
    revoked_at: null,
    revoked_reason: "",
    created_at: "2026-01-02T03:04:05Z",
    updated_at: "2026-02-03T04:05:06Z",
    ...overrides,
  };
}

const RUNNERS = [
  runnerRow({ assignable: true, not_assignable_code: "", is_busy: false, queue_depth: 0 }),
  runnerRow({
    id: RUNNER_MAC,
    name: "mac-mini",
    status: "offline",
    pod: POD_DEFAULT,
    last_heartbeat_at: null,
    owner: OWNER_ZOE,
    assignable: false,
    not_assignable_code: "runner_owner_not_on_issue",
    is_busy: false,
    queue_depth: 0,
  }),
];

function directoryRow(userId: string, displayName: string) {
  return {
    id: `${userId.slice(0, -1)}9`,
    member: {
      id: userId,
      first_name: displayName,
      last_name: "",
      avatar: "",
      avatar_url: null,
      display_name: displayName,
      is_bot: false,
    },
    role: 15,
    is_active: true,
  };
}

const DIRECTORY = [directoryRow(OWNER_ANA, "Ana"), directoryRow(OWNER_ZOE, "Zoe")];

interface ReadConfig {
  status?: number;
  body?: unknown;
  hang?: boolean;
}

function harness(pods: ReadConfig = {}, project: ReadConfig = {}, runners: ReadConfig = {}) {
  const calls: string[] = [];
  const transport = async (url: string) => {
    calls.push(url);
    if (url.endsWith("/auth/get-csrf-token/")) {
      return {
        status: 200,
        url,
        headers: { get: () => "application/json" },
        text: async () => '{"csrf_token":"csrf"}',
      };
    }
    const path = new URL(url).pathname;
    const route: Record<string, { config: ReadConfig; fallback: unknown }> = {
      "/api/runners/pods/": { config: pods, fallback: PODS },
      "/api/runners/": { config: runners, fallback: [] },
      "/api/workspaces/acme/members/": { config: {}, fallback: DIRECTORY },
      [`/api/workspaces/acme/projects/${PROJECT}/`]: { config: project, fallback: projectRow() },
    };
    const match = route[path] ?? { config: project, fallback: projectRow() };
    if (match.config.hang === true) await new Promise(() => {});
    const body = match.config.body === undefined ? match.fallback : match.config.body;
    return {
      status: match.config.status ?? 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    };
  };
  const client = createAppClient("https://api.test", transport as never);
  setSessionClient(client);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(["session", "workspaces"], WORKSPACES);
  return { queryClient, calls };
}

function optionTexts(): string[] {
  return within(screen.getByRole("listbox"))
    .getAllByRole("option")
    .map((row) => row.textContent ?? "");
}

function renderPicker(props: Partial<React.ComponentProps<typeof PodPicker>> = {}, queryClient?: QueryClient) {
  const client = queryClient ?? new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <PodPicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => {}} {...props} />
    </QueryClientProvider>
  );
  return client;
}

afterEach(() => {
  resetAppClient();
});

describe("PodPicker", () => {
  it("fetches lazily and lists agents plus pods with tags and reasons", async () => {
    const { queryClient, calls } = harness();
    renderPicker({}, queryClient);
    expect(calls).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("option", { name: "Pi Dash Cloud Agent" })).toBeInTheDocument();
    const texts = optionTexts();
    expect(texts).toHaveLength(4);
    expect(texts[0]).toContain("Pi Dash Cloud Agent");
    expect(texts[0]?.length).toBeGreaterThan("Pi Dash Cloud Agent".length);
    expect(texts[1]).toContain("Pi Dash Agent (desktop)");
    expect(texts[2]).toContain("ACME-Backend");
    expect(texts[2]).toMatch(/default/i);
    expect(texts[3]).toContain("ACME-Tier");
    expect(texts[3]).not.toMatch(/default/i);
    expect(calls.some((url) => url.includes("/api/runners/pods/"))).toBe(true);
    expect(calls.some((url) => url.endsWith(`/projects/${PROJECT}/`))).toBe(true);
    const runnersCall = calls.find((url) => new URL(url).pathname === "/api/runners/");
    expect(runnersCall).toBeDefined();
    expect(new URL(runnersCall ?? "").searchParams.get("assignable_for")).toBe("new");
  });

  it("disables unavailable agents and keeps them unselectable", async () => {
    const { queryClient } = harness();
    const onChange = vi.fn();
    renderPicker({ onChange }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    const cloud = await screen.findByRole("option", { name: "Pi Dash Cloud Agent" });
    const desktop = screen.getByRole("option", { name: "Pi Dash Agent (desktop)" });
    expect(cloud).toBeDisabled();
    expect(desktop).toBeDisabled();
    expect(cloud.textContent).toContain("No model is configured");
    expect(cloud.textContent).not.toContain("built-in");
    fireEvent.click(cloud);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("picks pods and available agents", async () => {
    const available = projectRow({
      agent_executor_options: [
        { kind: "cloud_agent", available: true, reason_code: "" },
        { kind: "managed_runner", available: true, reason_code: "" },
      ],
    });
    const { queryClient } = harness({}, { body: available });
    const onChange = vi.fn();
    renderPicker({ onChange }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("option", { name: "Any runner in ACME-Tier" }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({ type: "pod", podId: POD_TIER });
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("option", { name: "Pi Dash Cloud Agent" }));
    expect(onChange).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenCalledWith({ type: "agent", executor: "cloud_agent" });
  });

  it("searches pods and agents", async () => {
    const { queryClient } = harness();
    const user = userEvent.setup();
    renderPicker({}, queryClient);
    await user.click(screen.getByRole("button", { name: "Select target" }));
    await screen.findByRole("option", { name: "Any runner in ACME-Tier" });
    await user.type(screen.getByRole("combobox", { name: "Search execution targets" }), "tier");
    expect(optionTexts().some((text) => text.includes("ACME-Tier"))).toBe(true);
    expect(optionTexts().some((text) => /cloud agent/i.test(text))).toBe(false);
    await user.clear(screen.getByRole("combobox", { name: "Search execution targets" }));
    await user.type(screen.getByRole("combobox", { name: "Search execution targets" }), "default");
    expect(optionTexts().some((text) => text.includes("ACME-Backend"))).toBe(true);
    await user.clear(screen.getByRole("combobox", { name: "Search execution targets" }));
    await user.type(screen.getByRole("combobox", { name: "Search execution targets" }), "-no-such-target-");
    expect(screen.queryByRole("option")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("No matching results");
  });

  it("names the selection on the trigger", async () => {
    const { queryClient } = harness();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <PodPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={{ type: "pod", podId: POD_TIER } satisfies ExecutionSelection}
          onChange={() => {}}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select target" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    await screen.findByRole("option", { name: "Any runner in ACME-Tier" });
    expect(screen.getByRole("button", { name: "ACME-Tier" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <PodPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={{ type: "agent", executor: "cloud_agent" } satisfies ExecutionSelection}
          onChange={() => {}}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Pi Dash Cloud Agent" })).toBeInTheDocument();
  });

  it("falls back for unknown pods and local-only executors", async () => {
    const { queryClient } = harness();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <PodPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={{ type: "pod", podId: "99999999-9999-4999-8999-999999999999" } satisfies ExecutionSelection}
          onChange={() => {}}
        />
      </QueryClientProvider>
    );
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    await screen.findByRole("option", { name: "Any runner in ACME-Tier" });
    expect(screen.getByRole("button", { name: "Unknown pod" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <PodPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={{ type: "agent", executor: "local_runner" } satisfies ExecutionSelection}
          onChange={() => {}}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select target" })).toBeInTheDocument();
  });

  it("supports icon-only and custom triggers", () => {
    const { queryClient } = harness();
    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <PodPicker workspaceSlug="acme" projectId={PROJECT} value={null} onChange={() => {}} iconOnly />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Select target" })).toBeInTheDocument();
    rerender(
      <QueryClientProvider client={queryClient}>
        <PodPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={null}
          onChange={() => {}}
          trigger={<button type="button">Custom target</button>}
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Custom target" })).toBeInTheDocument();
  });

  it("shows the selection glyph on the icon-only trigger", async () => {
    const { queryClient } = harness();
    render(
      <QueryClientProvider client={queryClient}>
        <PodPicker
          workspaceSlug="acme"
          projectId={PROJECT}
          value={{ type: "agent", executor: "managed_runner" } satisfies ExecutionSelection}
          onChange={() => {}}
          iconOnly
        />
      </QueryClientProvider>
    );
    expect(screen.getByRole("button", { name: "Pi Dash Agent (desktop)" }).querySelector("svg")).not.toBeNull();
  });

  it("follows a controlled open state", async () => {
    const { queryClient } = harness();
    const onOpenChange = vi.fn();
    renderPicker({ open: true, onOpenChange }, queryClient);
    expect(await screen.findByRole("option", { name: "Any runner in ACME-Tier" })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("reports controlled closes without managing state", async () => {
    const { queryClient } = harness();
    const onOpenChange = vi.fn();
    const user = userEvent.setup();
    renderPicker({ open: true, onOpenChange }, queryClient);
    await screen.findByRole("option", { name: "Any runner in ACME-Tier" });
    await user.keyboard("{Escape}");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("does not fetch or open while disabled", () => {
    const { queryClient, calls } = harness();
    renderPicker({ disabled: true }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(calls).toEqual([]);
  });

  it("names a disabled row's target when preloaded", async () => {
    const { queryClient } = harness();
    renderPicker({ value: { type: "pod", podId: POD_TIER }, preload: true, disabled: true }, queryClient);
    expect(await screen.findByRole("button", { name: "ACME-Tier" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "ACME-Tier" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("shows the loading row while either read lags", async () => {
    const bothHang = harness({ hang: true }, { hang: true });
    renderPicker({}, bothHang.queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading execution targets");
  });

  it("stays loading while the project read lags behind pods", async () => {
    const { queryClient } = harness({}, { hang: true });
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading execution targets");
  });

  it("shows the error row when either read fails", async () => {
    const podsFail = harness({ status: 500 }, {});
    renderPicker({}, podsFail.queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load execution targets.");
  });

  it("shows the error row when the project read fails", async () => {
    const { queryClient } = harness({}, { status: 500 });
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load execution targets.");
  });

  it("preloads to name a pod before the popup opens", async () => {
    const { queryClient, calls } = harness();
    renderPicker({ value: { type: "pod", podId: POD_TIER }, preload: true }, queryClient);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "ACME-Tier" })).toBeInTheDocument();
    expect(calls.some((url) => url.includes("/api/runners/pods/"))).toBe(true);
    expect(calls.some((url) => url.endsWith(`/projects/${PROJECT}/`))).toBe(true);
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("names a preloaded agent without reading pods for its label", async () => {
    const { queryClient } = harness();
    renderPicker({ value: { type: "agent", executor: "cloud_agent" }, preload: true }, queryClient);
    expect(await screen.findByRole("button", { name: "Pi Dash Cloud Agent" })).toBeInTheDocument();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("lists each pod's runners under its squad row with status and owner", async () => {
    const { queryClient } = harness({}, {}, { body: RUNNERS });
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    const gpu = await screen.findByRole("option", { name: /gpu-box/ });
    expect(gpu.textContent).toContain("online · idle");
    await waitFor(() => expect(gpu.textContent).toContain("Ana"));
    const texts = optionTexts();
    const squad = texts.findIndex((text) => text.includes("Any runner in ACME-Backend"));
    const runnerRow = texts.findIndex((text) => text.includes("gpu-box"));
    const nextSquad = texts.findIndex((text) => text.includes("Any runner in ACME-Tier"));
    expect(squad).toBeGreaterThanOrEqual(0);
    expect(runnerRow).toBeGreaterThan(squad);
    expect(nextSquad).toBeGreaterThan(runnerRow);
  });

  it("disables unassignable runners with their reason and keeps them unselectable", async () => {
    const { queryClient } = harness({}, {}, { body: RUNNERS });
    const onChange = vi.fn();
    renderPicker({ onChange }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    const mac = await screen.findByRole("option", { name: /mac-mini/ });
    expect(mac).toBeDisabled();
    expect(mac.textContent).toContain("assignee");
    fireEvent.click(mac);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("picks a runner with its pod", async () => {
    const { queryClient } = harness({}, {}, { body: RUNNERS });
    const onChange = vi.fn();
    renderPicker({ onChange }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("option", { name: /gpu-box/ }));
    expect(onChange).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith({ type: "runner", runnerId: RUNNER_GPU, podId: POD_DEFAULT });
  });

  it("scopes the runner read to the issue", async () => {
    const { queryClient, calls } = harness({}, {}, { body: RUNNERS });
    renderPicker({ assignableFor: "issue-9" }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    await screen.findByRole("option", { name: /gpu-box/ });
    const runnersCall = calls.find((url) => new URL(url).pathname === "/api/runners/");
    expect(new URL(runnersCall ?? "").searchParams.get("assignable_for")).toBe("issue-9");
  });

  it("names a runner selection with its pod on the trigger", async () => {
    const { queryClient } = harness({}, {}, { body: RUNNERS });
    renderPicker({ value: { type: "runner", runnerId: RUNNER_GPU, podId: POD_DEFAULT }, preload: true }, queryClient);
    const trigger = await screen.findByRole("button", { name: /gpu-box/ });
    expect(trigger.textContent).toContain("gpu-box");
    expect(trigger.textContent).toContain("ACME-Backend");
    expect(trigger.querySelector("svg")).not.toBeNull();
  });

  it("falls back for unknown runners", async () => {
    const { queryClient } = harness({}, {}, { body: RUNNERS });
    renderPicker(
      {
        value: { type: "runner", runnerId: "99999999-9999-4999-8999-999999999999", podId: POD_DEFAULT },
        preload: true,
      },
      queryClient
    );
    expect(await screen.findByRole("button", { name: "Unknown runner" })).toBeInTheDocument();
  });

  it("shows the wait note for a busy runner selection", async () => {
    const busy = [runnerRow({ status: "busy", is_busy: true, queue_depth: 3, assignable: true })];
    const { queryClient } = harness({}, {}, { body: busy });
    renderPicker({ value: { type: "runner", runnerId: RUNNER_GPU, podId: POD_DEFAULT }, preload: true }, queryClient);
    expect(await screen.findByText(/will wait behind 3 others/)).toBeInTheDocument();
  });

  it("shows no wait note for a free runner selection", async () => {
    const { queryClient } = harness({}, {}, { body: RUNNERS });
    renderPicker({ value: { type: "runner", runnerId: RUNNER_GPU, podId: POD_DEFAULT }, preload: true }, queryClient);
    expect(await screen.findByRole("button", { name: /gpu-box/ })).toBeInTheDocument();
    expect(screen.queryByText(/will wait/)).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows the waiting badge and the save error on the trigger", async () => {
    const { queryClient } = harness({}, {}, { body: RUNNERS });
    renderPicker(
      {
        value: { type: "runner", runnerId: RUNNER_GPU, podId: POD_DEFAULT },
        preload: true,
        waitingLabel: "Offline",
        saveError: "That runner belongs to another project.",
      },
      queryClient
    );
    const trigger = await screen.findByRole("button", { name: /gpu-box/ });
    expect(trigger.textContent).toContain("Offline");
    expect(screen.getByRole("alert")).toHaveTextContent("That runner belongs to another project.");
  });

  it("shows the error row when the runner read fails", async () => {
    const { queryClient } = harness({}, {}, { status: 500 });
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load execution targets.");
  });

  it("stays loading while the runner read lags behind pods", async () => {
    const { queryClient } = harness({}, {}, { hang: true });
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading execution targets");
  });

  it("offers pods alone while the workspace id is unknown", async () => {
    const { queryClient, calls } = harness();
    queryClient.setQueryData(["session", "workspaces"], []);
    const onChange = vi.fn();
    renderPicker(
      { value: { type: "pod", podId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa" }, preload: true, onChange },
      queryClient
    );
    expect(await screen.findByRole("button", { name: "ACME-Backend" })).toBeInTheDocument();
    expect(calls.some((url) => new URL(url).pathname === "/api/runners/")).toBe(false);
    fireEvent.click(screen.getByRole("button", { name: "ACME-Backend" }));
    fireEvent.click(await screen.findByRole("option", { name: "Any runner in ACME-Tier" }));
    expect(onChange).toHaveBeenCalledWith({ type: "pod", podId: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb" });
  });
});
