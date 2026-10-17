// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { PodPicker } from "./PodPicker.js";
import type { ExecutionSelection } from "./pods.js";

const PROJECT = "22222222-2222-4222-8222-222222222222";
const WORKSPACE = "33333333-3333-4333-8333-333333333333";
const POD_DEFAULT = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const POD_TIER = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

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

interface ReadConfig {
  status?: number;
  body?: unknown;
  hang?: boolean;
}

function harness(pods: ReadConfig = {}, project: ReadConfig = {}) {
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
    const isPods = url.includes("/api/runners/pods/");
    const config = isPods ? pods : project;
    if (config.hang === true) await new Promise(() => {});
    const body = config.body === undefined ? (isPods ? PODS : projectRow()) : config.body;
    return {
      status: config.status ?? 200,
      url,
      headers: { get: () => "application/json" },
      text: async () => JSON.stringify(body),
    };
  };
  createAppClient("https://api.test", transport as never);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
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
    expect(cloud.textContent).toContain("AI provider");
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
    fireEvent.click(await screen.findByRole("option", { name: "ACME-Tier" }));
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
    await screen.findByRole("option", { name: "ACME-Tier" });
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
    await screen.findByRole("option", { name: "ACME-Tier" });
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
    await screen.findByRole("option", { name: "ACME-Tier" });
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
    expect(await screen.findByRole("option", { name: "ACME-Tier" })).toBeInTheDocument();
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("does not fetch or open while disabled", () => {
    const { queryClient, calls } = harness();
    renderPicker({ disabled: true }, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(calls).toEqual([]);
  });

  it("shows the loading row while either read lags", async () => {
    const bothHang = harness({ hang: true }, { hang: true });
    renderPicker({}, bothHang.queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading pods");
  });

  it("stays loading while the project read lags behind pods", async () => {
    const { queryClient } = harness({}, { hang: true });
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading pods");
  });

  it("shows the error row when either read fails", async () => {
    const podsFail = harness({ status: 500 }, {});
    renderPicker({}, podsFail.queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load pods.");
  });

  it("shows the error row when the project read fails", async () => {
    const { queryClient } = harness({}, { status: 500 });
    renderPicker({}, queryClient);
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't load pods.");
  });
});
