/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  connect: vi.fn(),
  fetchProject: vi.fn(),
  refresh: vi.fn(),
  params: {} as Record<string, string>,
  projectIds: [] as string[],
  getProject: vi.fn(),
}));

vi.mock("mobx-react", () => ({ observer: (component: unknown) => component }));
// Mirrors the real router shape: the root route match carries no params and
// the leaf match carries the accumulated ones. AgentRuntime renders on the
// root route, so a component that read params from its own route context
// would see `{}` here — exactly as it does in the app.
vi.mock("react-router", () => ({
  useMatches: () => [
    { id: "root", params: {} },
    { id: "leaf", params: mocks.params },
  ],
}));
vi.mock("@/hooks/store/user", () => ({ useUser: () => ({ data: { id: "user" } }) }));
vi.mock("@/hooks/store/use-project", () => ({
  useProject: () => ({
    fetchProjectDetails: mocks.fetchProject,
    workspaceProjectIds: mocks.projectIds,
    getProjectById: mocks.getProject,
  }),
}));
vi.mock("@/services/agent-runtime", () => ({
  connectAgentProject: mocks.connect,
  refreshAgentRuntime: mocks.refresh,
  resumeAgentRuntime: vi.fn(),
  isDesktop: () => true,
}));

import { AgentRuntime } from "../../core/components/agent-runtime";

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  mocks.params = { workspaceSlug: "workspace", projectId: "project" };
  mocks.projectIds = [];
  mocks.connect.mockResolvedValue(undefined);
  mocks.refresh.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("desktop project availability refresh", () => {
  it("enrolls from a direct issue link using its workspace project", async () => {
    mocks.params = { workspaceSlug: "workspace", workItem: "HELLOWORLD-1" };
    mocks.projectIds = ["hello-project"];
    mocks.getProject.mockReturnValue({ identifier: "HELLOWORLD" });
    mocks.fetchProject.mockResolvedValue({ agent_executor_options: [] });
    await act(async () => {
      render(<AgentRuntime />);
    });
    expect(mocks.connect).toHaveBeenCalledWith("workspace", "hello-project");
    expect(mocks.fetchProject).toHaveBeenCalledWith("workspace", "hello-project");
  });

  it("does not enroll an issue link until its workspace project is known", async () => {
    mocks.params = { workspaceSlug: "workspace", workItem: "HELLOWORLD-1" };
    mocks.projectIds = ["other-project"];
    mocks.getProject.mockReturnValue({ identifier: "OTHER" });
    await act(async () => {
      render(<AgentRuntime />);
    });
    expect(mocks.connect).not.toHaveBeenCalled();
  });

  it("refreshes the picker store after enrollment and the first heartbeat", async () => {
    mocks.fetchProject
      .mockResolvedValueOnce({ agent_executor_options: [{ kind: "managed_runner", available: false }] })
      .mockResolvedValue({ agent_executor_options: [{ kind: "managed_runner", available: true }] });
    await act(async () => {
      render(<AgentRuntime />);
    });
    expect(mocks.fetchProject).toHaveBeenCalledWith("workspace", "project");
    expect(screen.getByRole("status").textContent).toContain("Waiting for Pi Dash Agent");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(mocks.fetchProject).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("status").textContent).toContain("Runs on this computer");
  });

  it("does not claim connectivity when enrollment fails", async () => {
    mocks.connect.mockRejectedValue(new Error("Configure a model first"));
    await act(async () => {
      render(<AgentRuntime />);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(screen.getByRole("status").textContent).toContain("Configure a model first");
    expect(mocks.fetchProject).not.toHaveBeenCalled();
  });

  it("stops refreshing after leaving the project", async () => {
    mocks.fetchProject.mockResolvedValue({ agent_executor_options: [] });
    await act(async () => {
      render(<AgentRuntime />);
    });
    cleanup();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(mocks.fetchProject).toHaveBeenCalledTimes(1);
  });
});
