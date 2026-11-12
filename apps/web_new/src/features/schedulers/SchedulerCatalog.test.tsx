// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Scheduler } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";
import { ROLE_ADMIN, ROLE_GUEST, ROLE_MEMBER } from "../../core/session/index.js";
import { sessionKeys } from "../../core/session/queries.js";

const { contracts } = vi.hoisted(() => ({
  contracts: { deleteScheduler: vi.fn(), listSchedulers: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError } from "../../shared/shell/toaster.js";
import { SchedulerCatalog, formatSchedulerUpdatedAt, schedulerSourceLabel } from "./SchedulerCatalog.js";

const SLUG = "acme";

function row(overrides: Partial<Scheduler> = {}): Scheduler {
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
    updated_at: "2026-10-02T12:30:00Z",
    ...overrides,
  };
}

function setup(role: number, data?: Scheduler[]) {
  if (data !== undefined) contracts.listSchedulers.mockResolvedValue(data);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  queryClient.setQueryData(sessionKeys.workspaces, [
    { id: "w1", name: "Acme Workspace", slug: SLUG, logo_url: null, total_members: 3, role },
  ]);
  render(
    <QueryClientProvider client={queryClient}>
      <SchedulerCatalog workspaceSlug={SLUG} />
    </QueryClientProvider>
  );
  return queryClient;
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => resetAppClient());

describe("SchedulerCatalog", () => {
  it("formats source and timestamps, including invalid timestamps", () => {
    expect(schedulerSourceLabel("builtin")).toBe("Built in");
    expect(schedulerSourceLabel("manifest")).toBe("Manifest");
    expect(formatSchedulerUpdatedAt("bad")).toBe("Unknown");
    expect(formatSchedulerUpdatedAt("2026-10-02T12:30:00Z")).not.toBe("Unknown");
  });

  it("lists definition metadata and admin controls", async () => {
    setup(ROLE_ADMIN, [
      row(),
      row({
        id: "s2",
        name: "Imported",
        slug: "imported",
        source: "manifest",
        is_enabled: false,
        active_binding_count: 0,
      }),
    ]);
    const table = await screen.findByRole("table", { name: "Scheduler definitions" });
    expect(within(table).getByText("Weekly report")).toBeInTheDocument();
    expect(within(table).getByText("weekly-report")).toBeInTheDocument();
    expect(within(table).getByText("Built in")).toBeInTheDocument();
    expect(within(table).getByText("Manifest")).toBeInTheDocument();
    expect(within(table).getByText("Enabled")).toBeInTheDocument();
    expect(within(table).getByText("Disabled")).toBeInTheDocument();
    expect(within(table).getAllByTestId("scheduler-row")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "New scheduler" })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Edit" })).toHaveLength(2);
    expect(screen.getAllByRole("button", { name: "Install" })).toHaveLength(1);
    expect(document.title).toBe("Acme Workspace · Schedulers");
  });

  it("opens the bulk install dialog from an enabled definition", async () => {
    const user = userEvent.setup();
    setup(ROLE_ADMIN, [row()]);
    await screen.findByText("Weekly report");
    await user.click(screen.getByRole("button", { name: "Install" }));
    expect(screen.getByRole("dialog", { name: "Install Weekly report" })).toBeInTheDocument();
    expect(screen.getByText("One schedule applies to every selected project.")).toBeInTheDocument();
  });

  it.each([ROLE_MEMBER, ROLE_GUEST])("keeps role %s read-only", async (role) => {
    setup(role, [row()]);
    expect(await screen.findByText("Weekly report")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New scheduler" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
    expect(screen.queryByRole("columnheader", { name: "Actions" })).not.toBeInTheDocument();
  });

  it("guides admins and guests through an empty catalog", async () => {
    setup(ROLE_ADMIN, []);
    expect(await screen.findByText("No scheduler definitions yet")).toBeInTheDocument();
    expect(screen.getByText(/Create one/)).toBeInTheDocument();
    cleanup();
    setup(ROLE_GUEST, []);
    expect(await screen.findByText(/workspace admin can add/)).toBeInTheDocument();
  });

  it("falls back to the slug in the title when the workspace row is absent", async () => {
    contracts.listSchedulers.mockResolvedValue([]);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(sessionKeys.workspaces, []);
    render(
      <QueryClientProvider client={queryClient}>
        <SchedulerCatalog workspaceSlug={SLUG} />
      </QueryClientProvider>
    );
    await screen.findByText("No scheduler definitions yet");
    expect(document.title).toBe("acme · Schedulers");
  });

  it("opens create and edit dialogs", async () => {
    const user = userEvent.setup();
    setup(ROLE_ADMIN, [row()]);
    await screen.findByText("Weekly report");
    await user.click(screen.getByRole("button", { name: "New scheduler" }));
    expect(screen.getByRole("dialog", { name: "Create scheduler" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(screen.getByRole("button", { name: "Edit" }));
    expect(screen.getByRole("dialog", { name: "Edit scheduler" })).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveValue("Weekly report");
  });

  it("confirms delete, allows cancel, and completes removal", async () => {
    const user = userEvent.setup();
    contracts.deleteScheduler.mockResolvedValue(204);
    setup(ROLE_ADMIN, [row()]);
    await screen.findByText("Weekly report");
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("dialog")).toHaveTextContent("stops its project installs from firing");
    expect(screen.getByRole("dialog")).toHaveTextContent("weekly-report");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(contracts.deleteScheduler).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete scheduler" }));
    await waitFor(() => expect(contracts.deleteScheduler).toHaveBeenCalledWith(expect.anything(), SLUG, row().id));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("guards dismissal during deletion and stays open after failure", async () => {
    const user = userEvent.setup();
    let reject!: (error: unknown) => void;
    contracts.deleteScheduler.mockReturnValue(new Promise<never>((_resolve, fail) => (reject = fail)));
    setup(ROLE_ADMIN, [row()]);
    await screen.findByText("Weekly report");
    await user.click(screen.getByRole("button", { name: "Delete" }));
    await user.click(screen.getByRole("button", { name: "Delete scheduler" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Deleting…" })).toBeDisabled());
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    reject(new Error("Down"));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith("Could not delete scheduler", expect.any(String)));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("dismisses an idle delete dialog with Escape", async () => {
    const user = userEvent.setup();
    setup(ROLE_ADMIN, [row()]);
    await screen.findByText("Weekly report");
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(contracts.deleteScheduler).not.toHaveBeenCalled();
  });

  it("shows a loading placeholder", async () => {
    let resolve!: (value: Scheduler[]) => void;
    contracts.listSchedulers.mockReturnValueOnce(new Promise<Scheduler[]>((done) => (resolve = done)));
    setup(ROLE_ADMIN);
    expect(screen.getByRole("status", { name: "Loading schedulers" })).toBeInTheDocument();
    resolve([row()]);
    expect(await screen.findByText("Weekly report")).toBeInTheDocument();
  });

  it("waits for permissions before exposing the catalog", async () => {
    contracts.listSchedulers.mockResolvedValue([row()]);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <SchedulerCatalog workspaceSlug={SLUG} />
      </QueryClientProvider>
    );
    expect(screen.getByRole("status", { name: "Loading schedulers" })).toBeInTheDocument();
    expect(screen.queryByRole("table", { name: "Scheduler definitions" })).not.toBeInTheDocument();
  });

  it("shows a failed read and retries it", async () => {
    const user = userEvent.setup();
    contracts.listSchedulers.mockRejectedValueOnce(new Error("Offline")).mockResolvedValueOnce([]);
    setup(ROLE_ADMIN);
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Offline"));
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(contracts.listSchedulers).toHaveBeenCalledTimes(2));
    expect(await screen.findByText("No scheduler definitions yet")).toBeInTheDocument();
  });
});
