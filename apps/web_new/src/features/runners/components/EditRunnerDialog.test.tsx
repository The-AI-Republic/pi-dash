// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Runner edit dialog: the seed, a server-only rename, a machine-side model
// change confirmed by the daemon, the daemon's refusal, and the lock while
// the machine is offline.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { Runner, RunnerReconfigureStatus } from "@pidash/api-client";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const updateRunner = vi.fn(async (_input: unknown) => undefined);
const reconfigureRunner = vi.fn(async (_client: unknown, _id: string, _changes: unknown) => ({
  request_id: "33333333-3333-4333-8333-333333333333",
}));
const readStatus = vi.fn<(...args: unknown[]) => Promise<RunnerReconfigureStatus>>();
const notifySuccess = vi.fn();

vi.mock("../api/mutations.js", () => ({ useUpdateRunner: () => ({ mutateAsync: updateRunner }) }));
vi.mock("../addRunner/queries.js", () => ({ useRunnerModelCatalog: () => ({ data: undefined }) }));
vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({}) }));
vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return {
    ...original,
    reconfigureRunner: (...args: [unknown, string, unknown]) => reconfigureRunner(...args),
    getRunnerReconfigureStatus: (...args: unknown[]) => readStatus(...args),
  };
});
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifySuccess: (title: string) => notifySuccess(title) };
});

import { EDIT_RUNNER_OFFLINE } from "../edit/model.js";
import { EditRunnerDialog } from "./EditRunnerDialog.js";

vi.setConfig({ testTimeout: 20_000 });

const runner = (overrides: Partial<Runner> = {}): Runner =>
  ({
    id: "r-1",
    name: "alpha",
    status: "online",
    owner: "u-1",
    provisioning: "manual",
    revoked_at: null,
    dev_machine: "m-1",
    capabilities: ["agent:codex"],
    dev_metadata: { working_dir: "/w", model: "gpt-5.5", reasoning_effort: "high" },
    pod_detail: { id: "p-1", name: "WEB_default", is_default: true, project: "pr-1", project_identifier: "WEB" },
    ...overrides,
  }) as Runner;

function renderDialog(row: Runner, onClose = vi.fn()) {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <EditRunnerDialog workspaceSlug="acme" runner={row} onClose={onClose} />
    </QueryClientProvider>
  );
  return { onClose };
}

async function pick(label: string, option: string): Promise<void> {
  const trigger = screen.getByRole("button", { name: label });
  for (let attempt = 0; attempt < 3; attempt += 1) {
    await userEvent.click(trigger);
    const list = screen.queryByRole("listbox", { name: label });
    if (list !== null) {
      await userEvent.click(within(list).getByRole("option", { name: option }));
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  const list = await screen.findByRole("listbox", { name: label });
  await userEvent.click(within(list).getByRole("option", { name: option }));
}

beforeEach(() => {
  updateRunner.mockClear();
  reconfigureRunner.mockClear();
  readStatus.mockReset();
  notifySuccess.mockClear();
});

describe("EditRunnerDialog", () => {
  it("opens on the runner's current settings and its fixed project", () => {
    renderDialog(runner());
    expect(screen.getByLabelText("Name")).toHaveValue("alpha");
    expect(screen.getByLabelText(/Working directory/)).toHaveValue("/w");
    expect(screen.getByRole("button", { name: "Agent" })).toHaveTextContent("Codex");
    expect(screen.getByRole("button", { name: "Model" })).toHaveTextContent("GPT-5.5 (High)");
    expect(screen.getByText("WEB")).toBeInTheDocument();
  });

  it("renames on the server without touching the machine", async () => {
    const { onClose } = renderDialog(runner());
    const name = screen.getByLabelText("Name");
    await userEvent.clear(name);
    await userEvent.type(name, "beta");
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(updateRunner).toHaveBeenCalledWith({ runnerId: "r-1", patch: { name: "beta" } });
    expect(reconfigureRunner).not.toHaveBeenCalled();
  });

  it("applies a model change on the machine and waits for the daemon", async () => {
    readStatus.mockResolvedValueOnce({ request_id: "33333333-3333-4333-8333-333333333333", status: "ok" });
    const { onClose } = renderDialog(runner());
    await pick("Model", "GPT-5.4 (Low)");
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(reconfigureRunner).toHaveBeenCalledWith({}, "r-1", { model: "gpt-5.4", reasoning_effort: "low" });
    expect(notifySuccess).toHaveBeenCalled();
  });

  it("shows the daemon's refusal and stays open", async () => {
    readStatus.mockResolvedValueOnce({
      request_id: "33333333-3333-4333-8333-333333333333",
      status: "error",
      error: "the Claude Code CLI (claude) was not found on this machine; install it first",
    });
    const { onClose } = renderDialog(runner());
    await pick("Agent", "Claude Code");
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(await screen.findByTestId("edit-runner-error")).toHaveTextContent("not found on this machine");
    expect(reconfigureRunner).toHaveBeenCalledWith({}, "r-1", {
      agent: "claude-code",
      model: "",
      reasoning_effort: "",
    });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("locks the machine settings while the machine is offline, but still renames", () => {
    renderDialog(runner({ status: "offline" }));
    expect(screen.getByTestId("edit-runner-blocker")).toHaveTextContent(EDIT_RUNNER_OFFLINE);
    expect(screen.getByLabelText(/Working directory/)).toBeDisabled();
    expect(screen.getByRole("button", { name: "Agent" })).toBeDisabled();
    expect(screen.getByLabelText("Name")).not.toBeDisabled();
  });
});
