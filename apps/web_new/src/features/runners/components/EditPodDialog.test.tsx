// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Pod settings dialog (NEWFRONT-542, RUN-014). The name shows the suffix
// only, the form re-seeds per pod, saving sends the changed subset — or
// nothing when untouched — and the default switch is one-way.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ApiError, type Pod } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { contractMocks, notify } = vi.hoisted(() => ({
  contractMocks: { updatePod: vi.fn() },
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

import {
  EDIT_POD_DEFAULT_CURRENT,
  EDIT_POD_DEFAULT_HINT,
  EDIT_POD_DEFAULT_LABEL,
  EDIT_POD_HINT,
  EDIT_POD_SAVING,
  EDIT_POD_SUBMIT,
  EDIT_POD_TITLE,
  EditPodDialog,
} from "./EditPodDialog.js";
import { POD_ERROR_TITLE, POD_UPDATE_FAILED } from "../pods.js";

function pod(overrides: Partial<Pod> = {}): Pod {
  return {
    id: "pod-1",
    name: "WEB_beefy",
    description: "the big boxes",
    is_default: false,
    workspace: "ws-1",
    project: "prj-1",
    project_identifier: "WEB",
    created_by: null,
    runner_count: 1,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function setup(target: Pod | null, onClose: () => void = vi.fn()) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <EditPodDialog workspaceSlug="acme" pod={target} onClose={onClose} />
    </QueryClientProvider>
  );
  return { onClose, queryClient, ...view };
}

async function save(): Promise<void> {
  fireEvent.click(screen.getByRole("button", { name: EDIT_POD_SUBMIT }));
}

beforeEach(() => {
  for (const mock of Object.values(contractMocks)) mock.mockReset();
  for (const mock of Object.values(notify)) mock.mockReset();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => resetAppClient());

describe("EditPodDialog", () => {
  it("renders nothing without a pod", () => {
    setup(null);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("seeds the suffix, description and default state", async () => {
    setup(pod());
    expect(await screen.findByRole("heading", { name: EDIT_POD_TITLE })).toBeInTheDocument();
    expect(screen.getByText(EDIT_POD_HINT)).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveValue("beefy");
    expect(screen.getByLabelText("Description (optional)")).toHaveValue("the big boxes");
    const toggle = screen.getByRole("switch", { name: EDIT_POD_DEFAULT_LABEL });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(toggle).toBeEnabled();
    expect(screen.getByText(EDIT_POD_DEFAULT_HINT)).toBeInTheDocument();
  });

  it("disables the switch and explains it on the current default", async () => {
    setup(pod({ is_default: true }));
    const toggle = await screen.findByRole("switch", { name: EDIT_POD_DEFAULT_LABEL });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(toggle).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByText(EDIT_POD_DEFAULT_CURRENT)).toBeInTheDocument();
    expect(screen.queryByText(EDIT_POD_DEFAULT_HINT)).not.toBeInTheDocument();
  });

  it("re-seeds between pods so values never leak", async () => {
    const onClose = vi.fn();
    const first = pod();
    const queryClient = new QueryClient();
    const { rerender } = setup(first, onClose);
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "typed-but-unsaved" } });
    rerender(
      <QueryClientProvider client={queryClient}>
        <EditPodDialog workspaceSlug="acme" pod={null} onClose={onClose} />
      </QueryClientProvider>
    );
    rerender(
      <QueryClientProvider client={queryClient}>
        <EditPodDialog
          workspaceSlug="acme"
          pod={pod({
            id: "pod-2",
            name: "ARC_pinball",
            description: "",
            is_default: true,
            project_identifier: "ARC",
          })}
          onClose={onClose}
        />
      </QueryClientProvider>
    );
    await waitFor(() => expect(screen.getByLabelText("Name")).toHaveValue("pinball"));
    expect(screen.getByLabelText("Description (optional)")).toHaveValue("");
    expect(screen.getByRole("switch", { name: EDIT_POD_DEFAULT_LABEL })).toHaveAttribute("aria-checked", "true");
  });

  it("closes without a call for an untouched form", async () => {
    const { onClose } = setup(pod());
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "  beefy  " } });
    fireEvent.change(screen.getByLabelText("Description (optional)"), { target: { value: " the big boxes " } });
    await save();
    expect(contractMocks.updatePod).not.toHaveBeenCalled();
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("sends only the renamed suffix", async () => {
    const { onClose } = setup(pod());
    contractMocks.updatePod.mockResolvedValue(pod({ name: "WEB_heavier" }));
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: " heavier " } });
    await save();
    await waitFor(() =>
      expect(contractMocks.updatePod).toHaveBeenCalledWith(expect.anything(), "pod-1", { name: "heavier" })
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("sends only the description, including a cleared one", async () => {
    setup(pod());
    contractMocks.updatePod.mockResolvedValue(pod({ description: "" }));
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Description (optional)"), { target: { value: "   " } });
    await save();
    await waitFor(() =>
      expect(contractMocks.updatePod).toHaveBeenCalledWith(expect.anything(), "pod-1", { description: "" })
    );
  });

  it("sends only the default flag when promoted", async () => {
    setup(pod());
    contractMocks.updatePod.mockResolvedValue(pod({ is_default: true }));
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.click(screen.getByRole("switch", { name: EDIT_POD_DEFAULT_LABEL }));
    await save();
    await waitFor(() =>
      expect(contractMocks.updatePod).toHaveBeenCalledWith(expect.anything(), "pod-1", { is_default: true })
    );
  });

  it("refuses a blank name with its message and sends nothing", async () => {
    setup(pod());
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "   " } });
    await save();
    expect(await screen.findByText("Name is required.")).toBeInTheDocument();
    expect(contractMocks.updatePod).not.toHaveBeenCalled();
  });

  it("keeps the dialog open with a working save and blocks dismissal", async () => {
    const { onClose } = setup(pod());
    let resolve: ((value: unknown) => void) | undefined;
    contractMocks.updatePod.mockImplementation(
      () =>
        new Promise((done) => {
          resolve = done;
        })
    );
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "heavier" } });
    await save();
    expect(await screen.findByRole("button", { name: EDIT_POD_SAVING })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    await waitFor(() =>
      expect(screen.getByRole("switch", { name: EDIT_POD_DEFAULT_LABEL })).toHaveAttribute("aria-disabled", "true")
    );
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
    resolve?.(pod());
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("toasts the server's reason and stays open on failure", async () => {
    const { onClose } = setup(pod());
    contractMocks.updatePod.mockRejectedValue(
      new ApiError({ status: 400, code: "http", message: "that name is taken" })
    );
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "heavier" } });
    await save();
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(POD_ERROR_TITLE, "that name is taken"));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("falls back to the generic update message for other failures", async () => {
    setup(pod());
    contractMocks.updatePod.mockRejectedValue(new Error("offline"));
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "heavier" } });
    await save();
    await waitFor(() => expect(notify.notifyError).toHaveBeenCalledWith(POD_ERROR_TITLE, POD_UPDATE_FAILED));
  });

  it("closes on Escape while no save is in flight", async () => {
    const { onClose } = setup(pod());
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(contractMocks.updatePod).not.toHaveBeenCalled();
  });

  it("cancel closes without a call", async () => {
    const { onClose } = setup(pod());
    await screen.findByRole("heading", { name: EDIT_POD_TITLE });
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "heavier" } });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(contractMocks.updatePod).not.toHaveBeenCalled();
  });
});
