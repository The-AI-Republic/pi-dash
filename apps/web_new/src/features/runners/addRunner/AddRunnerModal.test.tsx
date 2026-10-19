// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Add-runner dialog (RUN-006–009, RUN-049). Covers both entries, project
// prefill+lock, validation, cascade, reset on reopen, machine auto-selection,
// gating, the manual command, the remote phases and poll cancellation.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import type { CreateRunnerStatus, DevMachine, Pod, Project } from "@pidash/api-client";
import { beforeEach, describe, expect, it, vi } from "vitest";

const area = {
  scope: { workspaceSlug: "acme", projectId: undefined as string | undefined },
  workspaceId: "ws-id",
  allowed: true,
};
vi.mock("../area.js", () => ({ useRunnersArea: () => area }));

const machineRows: DevMachine[] = [];
let machineData: DevMachine[] = machineRows;
const projectRows: Project[] = [];
const podRows: Pod[] = [];
const loadErrors = { machines: false, projects: false, pods: false };
const createRunner = vi.fn<(machineId: string, input: unknown) => Promise<{ request_id: string }>>();
const readStatus = vi.fn<(machineId: string, requestId: string, workspaceId: string) => Promise<CreateRunnerStatus>>();

vi.mock("./queries.js", () => ({
  useAddRunnerMachines: () => ({ data: loadErrors.machines ? undefined : machineData, isError: loadErrors.machines }),
  useAddRunnerProjects: () => ({ data: loadErrors.projects ? undefined : projectRows, isError: loadErrors.projects }),
  useAddRunnerPods: () => ({ data: loadErrors.pods ? undefined : podRows, isError: loadErrors.pods }),
  createRunnerTransport: () => ({ createRunner, readStatus }),
}));

const remoteCreate = vi.hoisted(() => ({ original: null as null | ((...args: never[]) => unknown) }));
const runRemoteCreateMock = vi.hoisted(() => vi.fn());
vi.mock("./remoteCreate.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("./remoteCreate.js")>();
  remoteCreate.original = original.runRemoteCreate as (...args: never[]) => unknown;
  return { ...original, runRemoteCreate: (...args: unknown[]) => runRemoteCreateMock(...args) };
});

import { DAEMON_TOO_OLD_TEXT, MANUAL_MACHINE_LABEL } from "./model.js";
import { AddRunnerModal } from "./AddRunnerModal.js";

// Coverage instrumentation makes picker interactions slower than the default
// 5s budget; the assertions are unchanged.
vi.setConfig({ testTimeout: 20_000 });

const PROJECT_ID = "11111111-1111-4111-8111-111111111111";
const PROJECT: Project = {
  id: PROJECT_ID,
  name: "Acme",
  identifier: "ACME",
  sort_order: null,
  logo_props: {},
  member_role: null,
  intake_count: 0,
  archived_at: null,
  workspace: "ws-id",
  cycle_view: false,
  issue_views_view: false,
  module_view: false,
  page_view: false,
  inbox_view: false,
  is_default: false,
  guest_view_all_features: false,
  project_lead: null,
  network: 2,
  created_at: "2024-01-01T00:00:00Z",
  updated_at: "2024-01-01T00:00:00Z",
  created_by: null,
  updated_by: null,
} as Project;
const OTHER_PROJECT: Project = {
  ...PROJECT,
  id: "22222222-2222-4222-8222-222222222222",
  name: "Beta",
  identifier: "BETA",
};

const machine = (overrides: Partial<DevMachine> = {}): DevMachine => ({
  id: "33333333-3333-4333-8333-333333333333",
  host_label: "host",
  label: "Laptop",
  visibility: 1,
  runner_count: 0,
  online_runner_count: 0,
  control_online: true,
  last_seen_at: null,
  last_heartbeat_at: null,
  revoked_at: null,
  created_at: "2024-01-01T00:00:00Z",
  updated_at: "2024-01-01T00:00:00Z",
  ...overrides,
});

const pod = (name: string, projectIdentifier: string): Pod => ({
  id: `id-${name}`,
  name,
  description: "",
  is_default: false,
  workspace: "ws-id",
  project: projectIdentifier === "ACME" ? PROJECT_ID : OTHER_PROJECT.id,
  project_identifier: projectIdentifier,
  created_by: null,
  runner_count: 0,
  created_at: "2024-01-01T00:00:00Z",
  updated_at: "2024-01-01T00:00:00Z",
});

function harness(props: { projectId?: string } = {}) {
  const onOpenChange = vi.fn();
  const view = render(<AddRunnerModal open onOpenChange={onOpenChange} {...props} />);
  return { onOpenChange, view };
}

async function pickPicker(label: string, option: string): Promise<void> {
  const trigger = screen.getByRole("button", { name: label });
  for (let attempt = 0; attempt < 3; attempt += 1) {
    await userEvent.click(trigger);
    const list = await screen.queryByRole("listbox", { name: label });
    if (list !== null) {
      await userEvent.click(within(list).getByRole("option", { name: option }));
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  const list = await screen.findByRole("listbox", { name: label });
  await userEvent.click(within(list).getByRole("option", { name: option }));
}

async function submit(): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: "Generate Runner" }));
}

beforeEach(() => {
  area.scope.projectId = undefined;
  machineRows.length = 0;
  machineData = machineRows;
  projectRows.length = 0;
  projectRows.push(PROJECT, OTHER_PROJECT);
  podRows.length = 0;
  loadErrors.machines = false;
  loadErrors.projects = false;
  loadErrors.pods = false;
  createRunner.mockReset();
  readStatus.mockReset();
  createRunner.mockResolvedValue({ request_id: "44444444-4444-4444-8444-444444444444" });
  runRemoteCreateMock.mockReset();
  runRemoteCreateMock.mockImplementation((...args: unknown[]) =>
    (remoteCreate.original as (...inner: unknown[]) => unknown)(...args)
  );
  readStatus.mockResolvedValue({ request_id: "44444444-4444-4444-8444-444444444444", status: "pending" });
});

describe("form entry (RUN-006)", () => {
  it("starts on the manual path with Claude Code's pinned model", () => {
    harness();
    expect(screen.getByRole("heading", { name: "Add runner" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent(MANUAL_MACHINE_LABEL);
    expect(screen.getByRole("button", { name: "Project" })).toHaveTextContent("Select a project");
    expect(screen.getByRole("button", { name: "Pod (optional)" })).toHaveTextContent("(default pod)");
    expect(screen.getByRole("button", { name: "Agent" })).toHaveTextContent("Claude Code");
    expect(screen.getByRole("button", { name: "Model (optional)" })).toHaveTextContent("Opus 4.8");
    expect(screen.getByLabelText("Name (optional)")).toHaveValue("");
  });

  it("requires a project before anything else", async () => {
    harness();
    await submit();
    expect(screen.getByRole("alert")).toHaveTextContent("Pick a project.");
  });

  it("rejects a bad name and accepts a good one", async () => {
    harness();
    await pickPicker("Project", "Acme");
    await userEvent.type(screen.getByLabelText("Name (optional)"), "has space", { timeout: 5_000 });
    await submit();
    expect(screen.getByRole("alert")).toHaveTextContent("Runner name cannot contain spaces.");
    await userEvent.clear(screen.getByLabelText("Name (optional)"));
    await userEvent.type(screen.getByLabelText("Name (optional)"), "good_name-1");
    await submit();
    expect(screen.getByText(/pidash runner add/)).toBeInTheDocument();
  });

  it("leaves the project blank when the locked id is not in the list", async () => {
    area.scope.projectId = "99999999-9999-4999-8999-999999999999";
    harness({ projectId: area.scope.projectId });
    await waitFor(() => expect(screen.getByRole("button", { name: "Project" })).toBeDisabled());
    expect(screen.getByRole("button", { name: "Project" })).toHaveTextContent("Select a project");
  });

  it("prefills and locks the project on a project-scoped entry", async () => {
    area.scope.projectId = PROJECT_ID;
    harness({ projectId: PROJECT_ID });
    await waitFor(() => expect(screen.getByRole("button", { name: "Project" })).toHaveTextContent("Acme"));
    expect(screen.getByRole("button", { name: "Project" })).toBeDisabled();
  });
});

describe("cascade (RUN-006)", () => {
  it("narrows pods to the project and clears an ill-fitting pick", async () => {
    podRows.push(pod("acme-pod", "ACME"), pod("beta-pod", "BETA"));
    harness();
    await pickPicker("Project", "Acme");
    await pickPicker("Pod (optional)", "acme-pod (ACME)");
    await pickPicker("Project", "Beta");
    expect(screen.getByRole("button", { name: "Pod (optional)" })).toHaveTextContent("(default pod)");
    await userEvent.click(screen.getByRole("button", { name: "Pod (optional)" }));
    const list = await screen.findByRole("listbox", { name: "Pod (optional)" });
    expect(within(list).queryByRole("option", { name: "acme-pod (ACME)" })).toBeNull();
    expect(within(list).getByRole("option", { name: "beta-pod (BETA)" })).toBeInTheDocument();
  });

  it("backfills the project from a pod-first pick", async () => {
    podRows.push(pod("beta-pod", "BETA"));
    harness();
    await pickPicker("Pod (optional)", "beta-pod (BETA)");
    await waitFor(() => expect(screen.getByRole("button", { name: "Project" })).toHaveTextContent("Beta"));
  });
});

describe("catalog (RUN-009, RUN-049)", () => {
  it("lists Opus 5.5 directly above Opus 5 and resets the model on agent change", async () => {
    harness();
    await userEvent.click(screen.getByRole("button", { name: "Model (optional)" }));
    let list = await screen.findByRole("listbox", { name: "Model (optional)" });
    const labels = within(list)
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(labels.indexOf("Opus 5.5")).toBe(labels.indexOf("Opus 5") - 1);
    await userEvent.click(within(list).getByRole("option", { name: "Opus 5.5" }));
    expect(screen.getByRole("button", { name: "Model (optional)" })).toHaveTextContent("Opus 5.5");
    await pickPicker("Agent", "Codex");
    expect(screen.getByRole("button", { name: "Model (optional)" })).toHaveTextContent(
      "Default (agent's built-in model)"
    );
  });

  it("emits --model claude-opus-5-5 in the manual command", async () => {
    harness();
    await pickPicker("Project", "Acme");
    await pickPicker("Model (optional)", "Opus 5.5");
    await submit();
    const command = screen.getByTestId("add-runner-command-text");
    expect(command).toHaveTextContent("--model claude-opus-5-5");
  });
});

describe("machine path (RUN-007, RUN-009)", () => {
  it("auto-selects the first connected machine and gates unsupported agents", async () => {
    machineRows.push(machine({ supported_agents: ["claude-code", "codex"] }));
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await userEvent.click(screen.getByRole("button", { name: "Agent" }));
    const list = await screen.findByRole("listbox", { name: "Agent" });
    expect(within(list).getByRole("option", { name: "Grok" })).toBeDisabled();
    expect(within(list).getByRole("option", { name: "Codex" })).not.toBeDisabled();
    expect(screen.getByText(/newer daemon/)).toBeInTheDocument();
  });

  it("snaps the agent when the picked machine cannot run it", async () => {
    machineRows.push(machine({ supported_agents: ["codex"] }));
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Dev machine", MANUAL_MACHINE_LABEL);
    expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent(MANUAL_MACHINE_LABEL);
    expect(screen.getByRole("button", { name: "Agent" })).toHaveTextContent("Claude Code");
  });

  it("shows load error banners without blocking the form", () => {
    loadErrors.machines = true;
    loadErrors.projects = true;
    loadErrors.pods = true;
    harness();
    expect(screen.getByText("Could not load dev machines.")).toBeInTheDocument();
    expect(screen.getByText("Could not load projects.")).toBeInTheDocument();
    expect(screen.getByText("Could not load pods.")).toBeInTheDocument();
  });

  it("gates nothing when the machine advertises no set", async () => {
    machineRows.push(machine());
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await userEvent.click(screen.getByRole("button", { name: "Agent" }));
    const list = await screen.findByRole("listbox", { name: "Agent" });
    expect(within(list).getByRole("option", { name: "Grok" })).not.toBeDisabled();
    expect(screen.queryByText(/newer daemon/)).toBeNull();
  });

  it("falls back to manual when the chosen machine goes offline", async () => {
    machineRows.push(machine());
    const { view } = harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    machineData = [machine({ control_online: false })];
    view.rerender(<AddRunnerModal open onOpenChange={() => undefined} />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent(MANUAL_MACHINE_LABEL)
    );
  });

  it("posts every optional field the form collected", async () => {
    machineRows.push(machine({ supported_agents: ["claude-code", "codex"] }));
    podRows.push(pod("acme-pod", "ACME"));
    readStatus.mockResolvedValue({
      request_id: "44444444-4444-4444-8444-444444444444",
      status: "ok",
      runner_name: "my-runner",
    });
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Project", "Acme");
    await pickPicker("Pod (optional)", "acme-pod (ACME)");
    await userEvent.type(screen.getByLabelText("Name (optional)"), "my-runner");
    await userEvent.type(screen.getByLabelText("Working directory (optional)"), "/work");
    await pickPicker("Agent", "Codex");
    await pickPicker("Model (optional)", "GPT-5.6 Sol (High)");
    await submit();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Runner created" })).toBeInTheDocument());
    expect(createRunner).toHaveBeenCalledWith(
      "33333333-3333-4333-8333-333333333333",
      expect.objectContaining({
        project: "ACME",
        pod: "acme-pod",
        name: "my-runner",
        working_dir: "/work",
        agent: "codex",
        model: "gpt-5.6-sol",
        reasoning_effort: "high",
      })
    );
  });

  it("posts the full form, polls, and reports success", async () => {
    machineRows.push(machine());
    readStatus.mockResolvedValue({
      request_id: "44444444-4444-4444-8444-444444444444",
      status: "ok",
      runner_name: "new-runner",
    });
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Project", "Acme");
    await userEvent.type(screen.getByLabelText("Name (optional)"), "my-runner");
    await pickPicker("Model (optional)", "Default (agent's built-in model)");
    await submit();
    await waitFor(() => expect(screen.getByRole("heading", { name: "Runner created" })).toBeInTheDocument());
    expect(screen.getByText("new-runner")).toBeInTheDocument();
    expect(createRunner).toHaveBeenCalledWith(
      "33333333-3333-4333-8333-333333333333",
      expect.objectContaining({ project: "ACME", name: "my-runner", agent: "claude-code" })
    );
    expect(createRunner.mock.calls[0]?.[1]).not.toHaveProperty("model");
    expect(createRunner.mock.calls[0]?.[1]).not.toHaveProperty("reasoning_effort");
    expect(readStatus).toHaveBeenCalled();
  });

  it("ignores a close request before the form has produced fields", async () => {
    machineRows.push(machine());
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    expect(screen.queryByRole("button", { name: "Show manual command" })).toBeNull();
  });

  it("reports a success without a runner name", async () => {
    machineRows.push(machine());
    readStatus.mockResolvedValue({
      request_id: "44444444-4444-4444-8444-444444444444",
      status: "ok",
    });
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Project", "Acme");
    await submit();
    expect(await screen.findByRole("heading", { name: "Runner created" })).toBeInTheDocument();
    expect(screen.getByText(/is now running on/)).toBeInTheDocument();
  });

  it("reports a failure with no detail", async () => {
    machineRows.push(machine());
    readStatus.mockResolvedValue({
      request_id: "44444444-4444-4444-8444-444444444444",
      status: "error",
    });
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Project", "Acme");
    await submit();
    expect(await screen.findByText("Runner creation failed.")).toBeInTheDocument();
  });

  it("carries the values into the manual command after a failure", async () => {
    machineRows.push(machine());
    readStatus.mockResolvedValue({
      request_id: "44444444-4444-4444-8444-444444444444",
      status: "error",
      error: 'unknown agent kind "muse-code"',
    });
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Project", "Acme");
    await submit();
    await waitFor(() => expect(screen.getByText(/Runner creation failed/)).toBeInTheDocument());
    expect(screen.getByText(new RegExp(DAEMON_TOO_OLD_TEXT.slice(0, 20)))).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Show manual command" }));
    expect(screen.getByTestId("add-runner-command-text")).toHaveTextContent("--project ACME");
  });

  it("times out when the machine never reports back", async () => {
    machineRows.push(machine());
    runRemoteCreateMock.mockImplementation(async (_input: unknown, deps: { update: (state: unknown) => void }) => {
      const state = { phase: "timeout", machineLabel: "Laptop" };
      deps.update(state);
      return state;
    });
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Project", "Acme");
    await submit();
    expect(await screen.findByText(/did not report back in time/)).toBeInTheDocument();
  });

  it("stops polling when the dialog closes during creation", async () => {
    machineRows.push(machine());
    const { onOpenChange } = harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Project", "Acme");
    let cancellationObserved = false;
    runRemoteCreateMock.mockImplementation(
      async (_input: unknown, deps: { update: (state: unknown) => void; isCancelled: () => boolean }) => {
        deps.update({ phase: "creating", machineLabel: "Laptop" });
        while (!deps.isCancelled()) await new Promise((resolve) => setTimeout(resolve, 1));
        cancellationObserved = true;
        return null;
      }
    );
    await submit();
    expect(await screen.findByText(/Creating runner/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    await waitFor(() => expect(cancellationObserved).toBe(true));
  });

  it("resets the screen when the dialog closes", async () => {
    const { view } = harness();
    await pickPicker("Project", "Acme");
    await submit();
    expect(screen.getByTestId("add-runner-command-text")).toBeInTheDocument();
    view.rerender(<AddRunnerModal open={false} onOpenChange={() => undefined} />);
    expect(screen.queryByTestId("add-runner-command-text")).toBeNull();
  });

  it("closes from the dialog dismiss control", async () => {
    const { onOpenChange } = harness();
    await userEvent.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("returns to the form from the failure panel", async () => {
    machineRows.push(machine());
    readStatus.mockResolvedValue({
      request_id: "44444444-4444-4444-8444-444444444444",
      status: "error",
      error: "boom",
    });
    harness();
    await waitFor(() => expect(screen.getByRole("button", { name: "Dev machine" })).toHaveTextContent("Laptop"));
    await pickPicker("Project", "Acme");
    await submit();
    await waitFor(() => expect(screen.getByText(/Runner creation failed/)).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("button", { name: "Generate Runner" })).toBeInTheDocument();
  });
});

describe("manual command (RUN-008)", () => {
  it("shows the command, switches shells, and returns to the form", async () => {
    harness();
    await pickPicker("Project", "Acme");
    await submit();
    const pre = screen.getByTestId("add-runner-command-text");
    expect(pre).toHaveTextContent("pidash runner add");
    expect(pre).toHaveTextContent("--project ACME");
    expect(screen.getByText(/VITE_API_BASE_URL/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "PowerShell" }));
    expect(screen.getByRole("button", { name: "PowerShell" })).toHaveAttribute("aria-pressed", "true");
    expect(pre.textContent).not.toContain("\n");
    await userEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("button", { name: "Generate Runner" })).toBeInTheDocument();
  });

  it("closes with Done", async () => {
    const { onOpenChange } = harness();
    await pickPicker("Project", "Acme");
    await submit();
    await userEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});

describe("configuredApiBase", () => {
  it("treats a missing or blank build value as unset", async () => {
    const original = (globalThis as { __PIDASH_API_BASE_URL__?: unknown }).__PIDASH_API_BASE_URL__;
    try {
      (globalThis as { __PIDASH_API_BASE_URL__?: unknown }).__PIDASH_API_BASE_URL__ = undefined;
      const { configuredApiBase } = await import("./AddRunnerModal.js");
      expect(configuredApiBase()).toBeNull();
      (globalThis as { __PIDASH_API_BASE_URL__?: unknown }).__PIDASH_API_BASE_URL__ = "   ";
      expect(configuredApiBase()).toBeNull();
      (globalThis as { __PIDASH_API_BASE_URL__?: unknown }).__PIDASH_API_BASE_URL__ = " https://api.test ";
      expect(configuredApiBase()).toBe("https://api.test");
    } finally {
      (globalThis as { __PIDASH_API_BASE_URL__?: unknown }).__PIDASH_API_BASE_URL__ = original;
    }
  });
});
