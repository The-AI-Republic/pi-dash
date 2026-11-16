// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Execution-pod selector (parity/specs/agents.md §4, AGT-019): the
// project default heads the list, a stale saved pod reads as the default,
// and the control is inert while the pods are loading.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Pod } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts } = vi.hoisted(() => ({ contracts: { listPods: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

import { DEFAULT_POD_VALUE, PodField, podLabel, selectedPodValue } from "./PodField.js";

function pod(overrides: Partial<Pod> = {}): Pod {
  return {
    id: "p1",
    name: "RKT runner pool",
    description: "",
    is_default: false,
    workspace: "w1",
    project: "proj1",
    project_identifier: "RKT",
    created_by: null,
    runner_count: 1,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-01T00:00:00Z",
    ...overrides,
  };
}

function setup(props: { value: string | null; disabled?: boolean }) {
  const onChange = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <PodField
        workspaceSlug="acme"
        projectId="proj1"
        value={props.value}
        onChange={onChange}
        {...(props.disabled === true ? { disabled: true } : {})}
      />
    </QueryClientProvider>
  );
  return { onChange, view, queryClient };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => {
  resetAppClient();
});

describe("PodField", () => {
  it("heads the list with the project default and marks the default pod", async () => {
    contracts.listPods.mockResolvedValue([pod({ id: "p0", name: "Main pool", is_default: true }), pod()]);
    setup({ value: null });
    await screen.findByRole("option", { name: "RKT runner pool" });
    expect(screen.getByLabelText("Pod")).toHaveValue(DEFAULT_POD_VALUE);
    const options = Array.from(screen.getByLabelText("Pod").querySelectorAll("option")).map(
      (option) => option.textContent
    );
    expect(options).toEqual(["Project default pod", "Main pool (default)", "RKT runner pool"]);
    expect(contracts.listPods).toHaveBeenCalledWith(expect.anything(), { projectId: "proj1" });
  });

  it("reports the picked pod and the default for an empty pick", async () => {
    contracts.listPods.mockResolvedValue([pod()]);
    const user = userEvent.setup();
    const { onChange } = setup({ value: null });
    await screen.findByRole("option", { name: "RKT runner pool" });
    await user.selectOptions(screen.getByLabelText("Pod"), "p1");
    expect(onChange).toHaveBeenLastCalledWith("p1");
    await user.selectOptions(screen.getByLabelText("Pod"), "");
    expect(onChange).toHaveBeenLastCalledWith(null);
  });

  it("reports the default once the saved pod is known to be gone", async () => {
    contracts.listPods.mockResolvedValue([pod()]);
    const { onChange } = setup({ value: "gone" });
    await screen.findByRole("option", { name: "RKT runner pool" });
    expect(screen.getByLabelText("Pod")).toHaveValue(DEFAULT_POD_VALUE);
    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("leaves a live saved pod and the default alone", async () => {
    contracts.listPods.mockResolvedValue([pod()]);
    const live = setup({ value: "p1" });
    await screen.findByRole("option", { name: "RKT runner pool" });
    const plain = setup({ value: null });
    await screen.findByRole("option", { name: "RKT runner pool" });
    expect(live.onChange).not.toHaveBeenCalled();
    expect(plain.onChange).not.toHaveBeenCalled();
  });

  it("does not report a change while the pods fail to load", async () => {
    contracts.listPods.mockRejectedValue(new Error("Offline"));
    const { onChange } = setup({ value: "gone" });
    await screen.findByText(/could not be loaded/);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("disables while the pods load and when the caller is busy", () => {
    contracts.listPods.mockReturnValue(new Promise(() => undefined));
    setup({ value: null });
    expect(screen.getByLabelText("Pod")).toBeDisabled();
  });

  it("explains a failed pod read and keeps the default usable", async () => {
    contracts.listPods.mockRejectedValue(new Error("Offline"));
    setup({ value: null, disabled: true });
    expect(await screen.findByText(/could not be loaded/)).toBeInTheDocument();
    expect(screen.getByLabelText("Pod")).toBeDisabled();
  });

  it("labels pods and resolves the selected value", () => {
    expect(podLabel(pod({ is_default: true }))).toBe("RKT runner pool (default)");
    expect(podLabel(pod())).toBe("RKT runner pool");
    expect(selectedPodValue("p1", [pod()])).toBe("p1");
    expect(selectedPodValue("p2", [pod()])).toBe(DEFAULT_POD_VALUE);
    expect(selectedPodValue(null, [])).toBe(DEFAULT_POD_VALUE);
  });
});
