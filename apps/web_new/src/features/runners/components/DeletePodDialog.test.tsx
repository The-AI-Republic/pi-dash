// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod deletion confirmation (NEWFRONT-542, RUN-015). The copy states the
// consequences up front; a refusal is translated by its guard code and
// keeps the dialog open; a delete in flight cannot be dismissed; an accepted
// delete reports its pod before closing.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ApiError, type Pod } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks, notify } = vi.hoisted(() => ({
  contractMocks: { deletePod: vi.fn() },
  notify: { notifyError: vi.fn(), notifySuccess: vi.fn(), notifyInfo: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, ...notify };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");

import { DELETE_POD_BODY, DELETE_POD_TITLE, DeletePodDialog } from "./DeletePodDialog.js";
import { POD_DELETE_FAILED, POD_DELETE_GUARDS, POD_ERROR_TITLE } from "../pods.js";

const TARGET: Pod = {
  id: "pod-1",
  name: "WEB_beefy",
  description: "",
  is_default: false,
  workspace: "ws-1",
  project: "prj-1",
  project_identifier: "WEB",
  created_by: null,
  runner_count: 0,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
};

function setup(pod: Pod | null) {
  const onCancel = vi.fn();
  const onDeleted = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <DeletePodDialog workspaceSlug="acme" pod={pod} onCancel={onCancel} onDeleted={onDeleted} />
    </QueryClientProvider>
  );
  return { onCancel, onDeleted };
}

function confirm(): void {
  fireEvent.click(screen.getByRole("button", { name: "Delete" }));
}

beforeEach(() => {
  for (const mock of Object.values(contractMocks)) mock.mockReset();
  for (const mock of Object.values(notify)) mock.mockReset();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("DeletePodDialog", () => {
  it("renders nothing without a pod", () => {
    setup(null);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("states the consequences and names the pod", async () => {
    setup(TARGET);
    expect(await screen.findByRole("heading", { name: DELETE_POD_TITLE })).toBeInTheDocument();
    expect(screen.getByText(DELETE_POD_BODY)).toHaveTextContent("unassigned");
    expect(screen.getByText(DELETE_POD_BODY)).toHaveTextContent("preserved");
    expect(screen.getByText("WEB_beefy")).toBeInTheDocument();
  });

  it("deletes the pod and reports it before closing", async () => {
    const { onCancel, onDeleted } = setup(TARGET);
    contractMocks.deletePod.mockResolvedValue(204);
    await screen.findByRole("heading", { name: DELETE_POD_TITLE });
    confirm();
    await waitFor(() => expect(contractMocks.deletePod).toHaveBeenCalledWith(expect.anything(), "pod-1"));
    await waitFor(() => expect(onDeleted).toHaveBeenCalledWith("pod-1"));
    expect(onCancel).toHaveBeenCalled();
    expect(onDeleted.mock.invocationCallOrder[0]).toBeLessThan(onCancel.mock.invocationCallOrder[0]!);
  });

  it("blocks both actions and dismissal while the delete is in flight", async () => {
    const { onCancel } = setup(TARGET);
    let resolve: ((value: unknown) => void) | undefined;
    contractMocks.deletePod.mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        })
    );
    await screen.findByRole("heading", { name: DELETE_POD_TITLE });
    confirm();
    await waitFor(() => expect(screen.getByRole("button", { name: "Delete" })).toBeDisabled());
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(onCancel).not.toHaveBeenCalled();
    resolve?.(204);
    await waitFor(() => expect(onCancel).toHaveBeenCalled());
  });

  it.each([
    ["pod_has_runners", "pod's runners"],
    ["pod_has_active_runs", "runs in flight"],
    ["default_pod_undeletable", "default pod"],
  ])("translates the %s refusal and keeps the dialog", async (code, fragment) => {
    const { onCancel, onDeleted } = setup(TARGET);
    contractMocks.deletePod.mockRejectedValue(new ApiError({ status: 409, code, message: "raw server text" }));
    await screen.findByRole("heading", { name: DELETE_POD_TITLE });
    confirm();
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(POD_ERROR_TITLE, POD_DELETE_GUARDS[code]));
    const shown = notify.notifyError.mock.calls[0]![1] as string;
    expect(shown).toContain(fragment);
    expect(shown).not.toContain("raw server text");
    expect(onDeleted).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: DELETE_POD_TITLE })).toBeInTheDocument();
  });

  it("shows the server's own sentence for an unrecognized refusal", async () => {
    setup(TARGET);
    contractMocks.deletePod.mockRejectedValue(new ApiError({ status: 403, code: "forbidden", message: "not yours" }));
    await screen.findByRole("heading", { name: DELETE_POD_TITLE });
    confirm();
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(POD_ERROR_TITLE, "not yours"));
  });

  it("falls back to the generic sentence for a transport failure", async () => {
    setup(TARGET);
    contractMocks.deletePod.mockRejectedValue(new Error("offline"));
    await screen.findByRole("heading", { name: DELETE_POD_TITLE });
    confirm();
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(POD_ERROR_TITLE, POD_DELETE_FAILED));
  });

  it("closes on Escape while no delete is in flight", async () => {
    const { onCancel } = setup(TARGET);
    await screen.findByRole("heading", { name: DELETE_POD_TITLE });
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await waitFor(() => expect(onCancel).toHaveBeenCalled());
    expect(contractMocks.deletePod).not.toHaveBeenCalled();
  });

  it("cancel closes without a delete", async () => {
    const { onCancel, onDeleted } = setup(TARGET);
    await screen.findByRole("heading", { name: DELETE_POD_TITLE });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalled();
    expect(contractMocks.deletePod).not.toHaveBeenCalled();
    expect(onDeleted).not.toHaveBeenCalled();
  });
});
