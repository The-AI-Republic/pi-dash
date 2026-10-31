// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ApiError, type Scheduler } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts } = vi.hoisted(() => ({
  contracts: { createScheduler: vi.fn(), updateScheduler: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/index.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { SchedulerDefinitionDialog } from "./SchedulerDefinitionDialog.js";

const SLUG = "acme";

function row(): Scheduler {
  return {
    id: "123e4567-e89b-12d3-a456-426614174000",
    workspace: "223e4567-e89b-12d3-a456-426614174000",
    slug: "weekly-report",
    name: "Weekly report",
    description: "Summary",
    prompt: "Review the week",
    color: "#5e6ad2",
    source: "builtin",
    is_enabled: true,
    active_binding_count: 2,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
  };
}

function setup(scheduler: Scheduler | null = null, open = true) {
  const onClose = vi.fn();
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <SchedulerDefinitionDialog workspaceSlug={SLUG} open={open} scheduler={scheduler} onClose={onClose} />
    </QueryClientProvider>
  );
  return { onClose };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => resetAppClient());

describe("SchedulerDefinitionDialog", () => {
  it("renders no form while closed", () => {
    setup(null, false);
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("derives the create handle until the user edits it", async () => {
    const user = userEvent.setup();
    setup();
    const name = screen.getByLabelText("Name");
    const handle = screen.getByLabelText("Handle");
    await user.type(name, "Weekly Report!");
    expect(handle).toHaveValue("weekly-report");
    await user.clear(handle);
    await user.type(handle, "Custom_Handle");
    expect(handle).toHaveValue("custom_handle");
    await user.clear(name);
    await user.type(name, "Changed Name");
    expect(handle).toHaveValue("custom_handle");
  });

  it("validates blank and malformed create values inline", async () => {
    const user = userEvent.setup();
    setup();
    await user.click(screen.getByRole("button", { name: "Create scheduler" }));
    expect(await screen.findByText("Enter a scheduler name.")).toBeInTheDocument();
    expect(screen.getByText("Enter a handle.")).toBeInTheDocument();
    expect(screen.getByText("Enter the base prompt.")).toBeInTheDocument();
    expect(contracts.createScheduler).not.toHaveBeenCalled();
    await user.type(screen.getByLabelText("Name"), "Name");
    await user.clear(screen.getByLabelText("Handle"));
    await user.type(screen.getByLabelText("Handle"), "bad_handle");
    await user.type(screen.getByLabelText("Base prompt"), "Prompt");
    await user.click(screen.getByRole("button", { name: "Create scheduler" }));
    expect(await screen.findByText("Use lowercase letters, numbers, and single dashes.")).toBeInTheDocument();
  });

  it("submits every create field and closes after success", async () => {
    const user = userEvent.setup();
    contracts.createScheduler.mockResolvedValue(row());
    const { onClose } = setup();
    await user.type(screen.getByLabelText("Name"), "Weekly report");
    await user.type(screen.getByLabelText("Description"), "Summary");
    await user.type(screen.getByLabelText("Base prompt"), "Review the week");
    await user.click(screen.getByRole("switch", { name: "Scheduler enabled" }));
    await user.click(screen.getByRole("button", { name: "Create scheduler" }));
    await waitFor(() => expect(contracts.createScheduler).toHaveBeenCalled());
    expect(contracts.createScheduler).toHaveBeenCalledWith(expect.anything(), SLUG, {
      name: "Weekly report",
      slug: "weekly-report",
      description: "Summary",
      prompt: "Review the week",
      color: "#5e6ad2",
      is_enabled: false,
    });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it("maps backend fields inline and stays open", async () => {
    const user = userEvent.setup();
    contracts.createScheduler.mockRejectedValue(
      new ApiError({
        status: 400,
        code: "http",
        message: "Rejected",
        fields: { slug: ["That handle is taken."], is_enabled: ["Cannot enable this definition."] },
      })
    );
    const { onClose } = setup();
    await user.type(screen.getByLabelText("Name"), "Weekly report");
    await user.type(screen.getByLabelText("Base prompt"), "Review");
    await user.click(screen.getByRole("button", { name: "Create scheduler" }));
    expect(await screen.findByText("That handle is taken.")).toBeInTheDocument();
    expect(screen.getByText("Cannot enable this definition.")).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("prefills edit, locks the handle, and omits it from the patch", async () => {
    const user = userEvent.setup();
    contracts.updateScheduler.mockResolvedValue({ ...row(), name: "Renamed" });
    const { onClose } = setup(row());
    expect(screen.getByLabelText("Handle")).toBeDisabled();
    expect(screen.getByLabelText("Handle")).toHaveValue("weekly-report");
    expect(screen.getByLabelText("Description")).toHaveValue("Summary");
    await user.clear(screen.getByLabelText("Name"));
    await user.type(screen.getByLabelText("Name"), "Renamed");
    await user.click(screen.getByRole("button", { name: "Save scheduler" }));
    await waitFor(() => expect(contracts.updateScheduler).toHaveBeenCalled());
    expect(contracts.updateScheduler).toHaveBeenCalledWith(expect.anything(), SLUG, row().id, {
      name: "Renamed",
      description: "Summary",
      prompt: "Review the week",
      color: "#5e6ad2",
      is_enabled: true,
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("accepts an edited color in the create payload", async () => {
    const user = userEvent.setup();
    contracts.createScheduler.mockResolvedValue(row());
    setup();
    await user.type(screen.getByLabelText("Name"), "Weekly report");
    await user.type(screen.getByLabelText("Base prompt"), "Review the week");
    const color = screen.getByLabelText("Color");
    fireEvent.change(color, { target: { value: "#112233" } });
    expect(color).toHaveValue("#112233");
    await user.click(screen.getByRole("button", { name: "Create scheduler" }));
    await waitFor(() => expect(contracts.createScheduler).toHaveBeenCalled());
    expect(contracts.createScheduler).toHaveBeenCalledWith(
      expect.anything(),
      SLUG,
      expect.objectContaining({ color: "#112233" })
    );
  });

  it("closes through cancel and the dialog close control", async () => {
    const user = userEvent.setup();
    const first = setup();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(first.onClose).toHaveBeenCalledTimes(1);
  });

  it("closes through the dialog close control", async () => {
    const user = userEvent.setup();
    const { onClose } = setup();
    await user.click(screen.getByRole("button", { name: "Close dialog" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
