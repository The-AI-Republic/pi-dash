// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Uninstall confirmation (parity/specs/agents.md §3, AGT-012): the dialog
// names the definition and the consequences, cancel keeps the install,
// confirm deletes the binding, a failure keeps the dialog, and the
// caller's removal callback drives the detail page back to the list.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, type SchedulerBinding } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts, notify } = vi.hoisted(() => ({
  contracts: { deleteSchedulerBinding: vi.fn(), listSchedulerBindings: vi.fn() },
  notify: { error: vi.fn(), success: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/index.js", () => ({ notifyError: notify.error, notifySuccess: notify.success }));

import { UninstallInstallDialog, uninstallConsequence } from "./UninstallInstallDialog.js";
import { bindingKeys } from "./installData.js";

const SLUG = "acme";
const PROJECT = "p1";

/** The fields every install carries until the run has happened. */
const NEVER_RAN = {
  next_run_at: null,
  last_run: null,
  last_run_status: null,
  last_run_ended_at: null,
  last_error: "",
  actor: null,
} as const;

const TIMESTAMPS = { created_at: "2026-10-01T00:00:00Z", updated_at: "2026-10-02T12:30:00Z" } as const;

const WEEKLY_SERIES = {
  dtstart: "2026-10-21T07:30:00Z",
  tzid: "UTC",
  rrule: "FREQ=WEEKLY",
  rdates: [] as string[],
  exdates: [] as string[],
  extra_context: "",
};

function binding(overrides: Partial<SchedulerBinding> = {}): SchedulerBinding {
  return {
    id: "b1",
    scheduler: "s1",
    scheduler_slug: "weekly-review",
    scheduler_name: "Weekly review",
    scheduler_color: "#22c55e",
    project: PROJECT,
    workspace: "w1",
    enabled: true,
    outcome_mode: "create_issue",
    pod: null,
    pod_name: null,
    ...WEEKLY_SERIES,
    ...NEVER_RAN,
    ...TIMESTAMPS,
    ...overrides,
  };
}

function setup(props: { binding: SchedulerBinding | null; onRemoved?: () => void }) {
  const onClose = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <UninstallInstallDialog
        workspaceSlug={SLUG}
        projectId={PROJECT}
        binding={props.binding}
        onClose={onClose}
        onRemoved={props.onRemoved}
      />
    </QueryClientProvider>
  );
  return { onClose, queryClient };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  contracts.deleteSchedulerBinding.mockResolvedValue(204);
});
afterEach(() => {
  resetAppClient();
});

describe("UninstallInstallDialog", () => {
  it("stays closed without a binding", () => {
    setup({ binding: null });
    expect(screen.queryByText("Uninstall scheduler?")).not.toBeInTheDocument();
  });

  it("explains both consequences and names the definition", () => {
    setup({ binding: binding() });
    expect(screen.getByRole("heading", { name: "Uninstall scheduler?" })).toBeInTheDocument();
    const text = uninstallConsequence(binding());
    expect(text).toContain("stops firing on this project");
    expect(text).toContain("unaffected");
    expect(screen.getByText(text)).toBeInTheDocument();
    expect(text).toContain("Weekly review");
  });

  it("cancel leaves the install alone", async () => {
    const user = userEvent.setup();
    const { onClose } = setup({ binding: binding() });
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(contracts.deleteSchedulerBinding).not.toHaveBeenCalled();
  });

  it("confirm deletes the binding, settles the list and hands back to the caller", async () => {
    const user = userEvent.setup();
    const onRemoved = vi.fn();
    const { onClose, queryClient } = setup({ binding: binding(), onRemoved });
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    queryClient.setQueryData(bindingKeys.list(SLUG, PROJECT), [binding()]);
    await user.click(screen.getByRole("button", { name: "Uninstall" }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(contracts.deleteSchedulerBinding).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "b1");
    expect(notify.success).toHaveBeenCalledWith(
      "Scheduler uninstalled",
      "It stops firing on this project until reinstalled."
    );
    expect(onRemoved).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: bindingKeys.list(SLUG, PROJECT) });
  });

  it("keeps the dialog and its explanation when removal fails", async () => {
    const user = userEvent.setup();
    contracts.deleteSchedulerBinding.mockRejectedValue(
      new ApiError({ status: 404, code: "not_found", message: "No such install." })
    );
    const onRemoved = vi.fn();
    const { onClose } = setup({ binding: binding(), onRemoved });
    await user.click(screen.getByRole("button", { name: "Uninstall" }));
    await waitFor(() => expect(notify.error).toHaveBeenCalledWith("Something went wrong", "No such install."));
    expect(onClose).not.toHaveBeenCalled();
    expect(onRemoved).not.toHaveBeenCalled();
    expect(screen.getByRole("heading", { name: "Uninstall scheduler?" })).toBeInTheDocument();
  });

  it("cannot be dismissed mid-flight and dismisses from Escape when idle", async () => {
    const user = userEvent.setup();
    let resolve: (value: number) => void = () => undefined;
    contracts.deleteSchedulerBinding.mockReturnValue(
      new Promise<number>((settle) => {
        resolve = settle;
      })
    );
    const { onClose } = setup({ binding: binding() });
    await user.click(screen.getByRole("button", { name: "Uninstall" }));
    await user.keyboard("{Escape}");
    expect(onClose).not.toHaveBeenCalled();
    resolve(204);
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("dismisses an idle dialog from Escape", async () => {
    const user = userEvent.setup();
    const { onClose } = setup({ binding: binding() });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("blocks a second confirm while the first is in flight", async () => {
    const user = userEvent.setup();
    let resolve: (value: number) => void = () => undefined;
    contracts.deleteSchedulerBinding.mockReturnValue(
      new Promise<number>((settle) => {
        resolve = settle;
      })
    );
    setup({ binding: binding() });
    await user.click(screen.getByRole("button", { name: "Uninstall" }));
    const busy = await screen.findByRole("button", { name: "Uninstalling…" });
    expect(busy).toBeDisabled();
    await user.click(busy);
    expect(contracts.deleteSchedulerBinding).toHaveBeenCalledTimes(1);
    resolve(204);
    await waitFor(() => expect(notify.success).toHaveBeenCalled());
  });
});
