// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import type { AgentExecutorOption, PodList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import {
  buildExecutionTargets,
  executionTargetLabel,
  executionTargetPatch,
  formatAvailabilityReason,
  selectDefaultPod,
  useExecutionTargets,
} from "./podOptions.js";

const POD_A = {
  id: "123e4567-e89b-12d3-a456-426614174000",
  name: "ACME-Backend",
  description: "",
  is_default: true,
  workspace: "323e4567-e89b-12d3-a456-426614174002",
  project: "223e4567-e89b-12d3-a456-426614174001",
  project_identifier: "ACME",
  created_by: null,
  runner_count: 1,
  created_at: "2026-01-02T03:04:05Z",
  updated_at: "2026-02-03T04:05:06Z",
};

const POD_B = { ...POD_A, id: "423e4567-e89b-12d3-a456-426614174003", name: "ACME-Tier", is_default: false };

function option(kind: string, available: boolean, reason_code: string): AgentExecutorOption {
  return { kind, available, reason_code };
}

describe("formatAvailabilityReason", () => {
  it("renders every known code as a sentence", () => {
    expect(formatAvailabilityReason("")).toBe("");
    expect(formatAvailabilityReason("cloud_agent_unavailable")).toContain("Cloud Agent");
    expect(formatAvailabilityReason("llm_config_missing")).toContain("AI provider");
    expect(formatAvailabilityReason("no_local_runner")).toContain("runner");
    expect(formatAvailabilityReason("managed_runner_disabled")).toContain("disabled");
    expect(formatAvailabilityReason("desktop_not_connected")).toContain("not connected");
    expect(formatAvailabilityReason("gateway_scopes_missing")).toContain("reconnecting");
    expect(formatAvailabilityReason("byok_not_supported_on_desktop")).toContain("BYOK");
    expect(formatAvailabilityReason("no_managed_runner_for_project")).toContain("enrolled");
  });

  it("surfaces unknown codes as-is", () => {
    expect(formatAvailabilityReason("tomorrow_problem")).toBe("tomorrow_problem");
  });
});

describe("buildExecutionTargets", () => {
  it("lists offered agent rows first, then pods in server order", () => {
    expect(
      buildExecutionTargets([POD_A, POD_B] as PodList, [
        option("cloud_agent", true, ""),
        option("local_runner", false, "no_local_runner"),
        option("managed_runner", false, "desktop_not_connected"),
      ])
    ).toEqual([
      { kind: "agent", executor: "cloud_agent", label: "Pi Dash Cloud Agent", available: true, reason: "" },
      {
        kind: "agent",
        executor: "managed_runner",
        label: "Pi Dash Agent (desktop)",
        available: false,
        reason: "Your desktop app is not connected.",
      },
      { kind: "pod", podId: POD_A.id, label: "ACME-Backend", isDefault: true },
      { kind: "pod", podId: POD_B.id, label: "ACME-Tier", isDefault: false },
    ]);
  });

  it("renders no row for an agent the config does not offer", () => {
    expect(buildExecutionTargets([POD_A] as PodList, [option("managed_runner", true, "")])).toEqual([
      { kind: "agent", executor: "managed_runner", label: "Pi Dash Agent (desktop)", available: true, reason: "" },
      { kind: "pod", podId: POD_A.id, label: "ACME-Backend", isDefault: true },
    ]);
    expect(buildExecutionTargets([] as PodList, [])).toEqual([]);
  });
});

describe("selectDefaultPod", () => {
  it("picks the default pod, if one exists", () => {
    expect(selectDefaultPod([POD_B, POD_A] as PodList)).toMatchObject({ id: POD_A.id });
    expect(selectDefaultPod([POD_B] as PodList)).toBe(null);
    expect(selectDefaultPod([] as PodList)).toBe(null);
  });
});

describe("executionTargetLabel", () => {
  it("reads Runs on once a cloud agent is offered, else Pod", () => {
    expect(executionTargetLabel([option("cloud_agent", false, "llm_config_missing")])).toBe("Runs on");
    expect(executionTargetLabel([option("managed_runner", true, "")])).toBe("Pod");
    expect(executionTargetLabel([])).toBe("Pod");
  });
});

describe("executionTargetPatch", () => {
  it("pins the pod plus the local executor on a pod pick", () => {
    expect(executionTargetPatch({ type: "pod", podId: POD_B.id })).toEqual({
      assigned_pod_id: POD_B.id,
      agent_executor: "local_runner",
    });
  });

  it("pins the executor and clears the pod on an agent pick", () => {
    expect(executionTargetPatch({ type: "agent", executor: "cloud_agent" })).toEqual({
      assigned_pod_id: null,
      agent_executor: "cloud_agent",
    });
    expect(executionTargetPatch({ type: "agent", executor: "managed_runner" })).toEqual({
      assigned_pod_id: null,
      agent_executor: "managed_runner",
    });
  });
});

function Probe({ slug, projectId }: { slug: string; projectId: string }): React.ReactElement {
  const { pods, options, isPending, error } = useExecutionTargets(slug, projectId);
  return (
    <span
      data-testid="execution-targets"
      data-pending={isPending ? "true" : "false"}
      data-error={error === null ? "none" : "yes"}
    >
      {`${pods?.length ?? "no-pods"}/${options?.length ?? "no-options"}`}
    </span>
  );
}

describe("useExecutionTargets", () => {
  afterEach(() => {
    resetAppClient();
  });

  function renderProbe(queryClient: QueryClient): void {
    render(
      <QueryClientProvider client={queryClient}>
        <Probe slug="acme" projectId="project-1" />
      </QueryClientProvider>
    );
  }

  it("serves cached pods plus executor options", () => {
    createAppClient("https://api.test", (() => {
      throw new Error("must not fetch: targets are cached");
    }) as never);
    const queryClient = new QueryClient();
    queryClient.setQueryData(issueKeys.pods("acme", "project-1"), [POD_A, POD_B]);
    queryClient.setQueryData(issueKeys.project("acme", "project-1"), {
      agent_executor_options: [option("cloud_agent", true, "")],
    });
    renderProbe(queryClient);
    const probe = screen.getByTestId("execution-targets");
    expect(probe).toHaveTextContent("2/1");
    expect(probe).toHaveAttribute("data-pending", "false");
    expect(probe).toHaveAttribute("data-error", "none");
  });

  it("stays pending while either read loads", () => {
    createAppClient("https://api.test", (async () => {
      throw new Error("offline");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(issueKeys.pods("acme", "project-1"), [POD_A]);
    renderProbe(queryClient);
    const probe = screen.getByTestId("execution-targets");
    expect(probe).toHaveAttribute("data-pending", "true");
    expect(probe).toHaveTextContent("1/no-options");
  });

  it("surfaces the project failure while serving cached pods", async () => {
    const silence = vi.spyOn(console, "error").mockImplementation(() => {});
    createAppClient("https://api.test", (async () => {
      throw new Error("offline");
    }) as never);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(issueKeys.pods("acme", "project-1"), [POD_A]);
    renderProbe(queryClient);
    await waitFor(() => expect(screen.getByTestId("execution-targets")).toHaveAttribute("data-error", "yes"));
    expect(screen.getByTestId("execution-targets")).toHaveTextContent("1/no-options");
    silence.mockRestore();
  });

  it("surfaces the pods failure when both reads fail", async () => {
    const silence = vi.spyOn(console, "error").mockImplementation(() => {});
    createAppClient("https://api.test", (async () => {
      throw new Error("offline");
    }) as never);
    renderProbe(new QueryClient({ defaultOptions: { queries: { retry: false } } }));
    await waitFor(() => expect(screen.getByTestId("execution-targets")).toHaveAttribute("data-error", "yes"));
    expect(screen.getByTestId("execution-targets")).toHaveTextContent("no-pods/no-options");
    silence.mockRestore();
  });
});
