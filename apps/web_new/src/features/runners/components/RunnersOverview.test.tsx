// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Overview table and row actions (NEWFRONT-540, RUN-002/RUN-010/RUN-011).
// Seven columns, dash fallbacks, revoke only where credentials exist, a
// Details route per row, and the two confirmations with their failure toast.
import type { Runner } from "@pidash/api-client";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const navigate = vi.fn(async () => undefined);
const notifyError = vi.fn();

vi.mock("@tanstack/react-router", async (importOriginal) => {
  const original = await importOriginal<typeof import("@tanstack/react-router")>();
  return { ...original, useNavigate: () => navigate };
});
vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: (title: string, description?: string) => notifyError(title, description) };
});

import type { PodFilterState } from "../podFilter.js";
import {
  canRevokeRunner,
  DELETE_RUNNER_FAILED,
  REVOKE_RUNNER_FAILED,
  RUNNERS_EMPTY_TEXT,
  RUNNER_ACTION_FAILED_TITLE,
  RunnersOverview,
  runnerHeartbeat,
  runnerPlatform,
  runnerVersion,
} from "./RunnersOverview.js";

const runner = (overrides: Partial<Runner> = {}): Runner =>
  ({
    id: "r-1",
    name: "alpha-runner",
    status: "online",
    os: "linux",
    arch: "arm64",
    runner_version: "0.1.0",
    last_heartbeat_at: "2024-01-02T03:04:05Z",
    pod: "p-1",
    pod_detail: { id: "p-1", name: "Alpha pod" },
    enrolled_at: "2024-01-01T00:00:00Z",
    ...overrides,
  }) as Runner;

function state(runners: Runner[]): PodFilterState {
  return { selectedPodId: null, selectedPod: null, runners, toggle: vi.fn(), clear: vi.fn() };
}

function renderTable(
  runners: Runner[],
  handlers: { onDelete?: (id: string) => Promise<void>; onRevoke?: (id: string) => Promise<void> } = {}
) {
  const onDelete = handlers.onDelete ?? vi.fn(async () => undefined);
  const onRevoke = handlers.onRevoke ?? vi.fn(async () => undefined);
  render(
    <RunnersOverview
      scope={{ workspaceSlug: "acme" }}
      filter={state(runners)}
      onDelete={onDelete}
      onRevoke={onRevoke}
    />
  );
  return { onDelete, onRevoke };
}

beforeEach(() => {
  navigate.mockClear();
  notifyError.mockClear();
});

describe("cell helpers", () => {
  it("formats the platform cell with a dash fallback", () => {
    expect(runnerPlatform({ os: "linux", arch: "arm64" })).toBe("linux / arm64");
    expect(runnerPlatform({ os: "", arch: "" })).toBe("—");
  });

  it("formats the version cell with a dash fallback", () => {
    expect(runnerVersion({ runner_version: "1.2.3" })).toBe("1.2.3");
    expect(runnerVersion({ runner_version: "" })).toBe("—");
  });

  it("localizes the heartbeat and dashes a never-seen runner", () => {
    expect(runnerHeartbeat({ last_heartbeat_at: "2024-01-02T03:04:05Z" }, () => "local")).toBe("local");
    expect(runnerHeartbeat({ last_heartbeat_at: null })).toBe("—");
  });

  it("offers revoke only while credentials exist", () => {
    expect(canRevokeRunner(runner())).toBe(true);
    expect(canRevokeRunner(runner({ enrolled_at: null }))).toBe(false);
    expect(canRevokeRunner(runner({ status: "revoked" }))).toBe(false);
  });
});

describe("RunnersOverview table", () => {
  it("renders seven columns and one row per runner", () => {
    renderTable([
      runner(),
      runner({ id: "r-2", name: "beta-runner", status: "busy", pod_detail: { id: "p-2", name: "Beta pod" } as never }),
    ]);
    const table = screen.getByRole("table");
    expect(
      within(table)
        .getAllByRole("columnheader")
        .map((cell) => cell.textContent)
    ).toEqual(["Name", "Pod", "Status", "OS / Arch", "Version", "Last heartbeat", "Actions"]);
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    expect(
      within(rows[0]!)
        .getAllByRole("cell")
        .map((cell) => cell.textContent)
    ).toEqual([
      "alpha-runner",
      "Alpha pod",
      "online",
      "linux / arm64",
      "0.1.0",
      new Date("2024-01-02T03:04:05Z").toLocaleString(),
      "DetailsRevokeDelete",
    ]);
    expect(within(rows[1]!).getAllByRole("cell")[2]).toHaveTextContent("busy");
  });

  it("shows the empty message in a single full-width cell", () => {
    renderTable([]);
    const rows = within(screen.getByRole("table")).getAllByRole("row");
    expect(rows).toHaveLength(2);
    expect(rows[1]).toHaveTextContent(RUNNERS_EMPTY_TEXT);
    expect(within(rows[1]!).getAllByRole("cell")).toHaveLength(1);
  });

  it("hides revoke for pending-enrolment and revoked rows", () => {
    renderTable([
      runner({ id: "r-p", name: "pending-runner", enrolled_at: null }),
      runner({ id: "r-v", name: "revoked-runner", status: "revoked" }),
    ]);
    const rows = screen.getAllByRole("row").slice(1);
    for (const row of rows) {
      expect(within(row).queryByRole("button", { name: "Revoke" })).toBeNull();
      expect(within(row).getByRole("button", { name: "Delete" })).toBeInTheDocument();
    }
  });

  it("reads a never-enrolled row as offline", () => {
    renderTable([runner({ enrolled_at: null, status: "online" })]);
    expect(screen.getAllByRole("row")[1]).toHaveTextContent("offline");
  });

  it("navigates to the detail route from the row", async () => {
    renderTable([runner()]);
    await userEvent.click(screen.getByRole("button", { name: "Details" }));
    expect(navigate).toHaveBeenCalledWith({ to: "/acme/runners/detail/r-1" });
  });

  it("builds project detail links under the project base", async () => {
    render(
      <RunnersOverview
        scope={{ workspaceSlug: "acme", projectId: "p-9" }}
        filter={state([runner()])}
        onDelete={vi.fn(async () => undefined)}
        onRevoke={vi.fn(async () => undefined)}
      />
    );
    await userEvent.click(screen.getByRole("button", { name: "Details" }));
    expect(navigate).toHaveBeenCalledWith({ to: "/acme/projects/p-9/runners/detail/r-1" });
  });
});

describe("RunnersOverview pod filter", () => {
  it("shows the active-pod bar and clears it through the affordance", async () => {
    const clear = vi.fn();
    render(
      <RunnersOverview
        scope={{ workspaceSlug: "acme" }}
        filter={{
          selectedPodId: "p-1",
          selectedPod: {
            id: "p-1",
            name: "Alpha pod",
            description: "",
            is_default: false,
            workspace: "ws-id",
            project: "p-1",
            project_identifier: "ALP",
            created_by: null,
            runner_count: 1,
            created_at: "2024-01-01T00:00:00Z",
            updated_at: "2024-01-01T00:00:00Z",
          },
          runners: [runner()],
          toggle: vi.fn(),
          clear,
        }}
        onDelete={vi.fn(async () => undefined)}
        onRevoke={vi.fn(async () => undefined)}
      />
    );
    expect(screen.getByText("Filtering runners by pod Alpha pod")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Clear filter" }));
    expect(clear).toHaveBeenCalledTimes(1);
  });
});

describe("RunnersOverview row actions", () => {
  it("deletes through the confirmation and closes it on success", async () => {
    const { onDelete } = renderTable([runner()]);
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete runner?" })).toBeInTheDocument();
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(onDelete).toHaveBeenCalledWith("r-1"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("revokes through the confirmation", async () => {
    const { onRevoke, onDelete } = renderTable([runner()]);
    await userEvent.click(screen.getByRole("button", { name: "Revoke" }));
    expect(screen.getByRole("dialog", { name: "Revoke runner?" })).toBeInTheDocument();
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Revoke" }));
    await waitFor(() => expect(onRevoke).toHaveBeenCalledWith("r-1"));
    expect(onDelete).not.toHaveBeenCalled();
  });

  it("toasts a failed delete and leaves the confirmation open", async () => {
    renderTable([runner()], {
      onDelete: vi.fn(async () => {
        throw new Error("boom");
      }),
    });
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(RUNNER_ACTION_FAILED_TITLE, DELETE_RUNNER_FAILED));
    expect(screen.getByRole("dialog", { name: "Delete runner?" })).toBeInTheDocument();
    // The action recovers: the confirm is usable for a retry.
    await waitFor(() =>
      expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" })).toBeEnabled()
    );
  });

  it("toasts a failed revoke", async () => {
    renderTable([runner()], {
      onRevoke: vi.fn(async () => {
        throw new Error("boom");
      }),
    });
    await userEvent.click(screen.getByRole("button", { name: "Revoke" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Revoke" }));
    await waitFor(() => expect(notifyError).toHaveBeenCalledWith(RUNNER_ACTION_FAILED_TITLE, REVOKE_RUNNER_FAILED));
    expect(screen.getByRole("dialog", { name: "Revoke runner?" })).toBeInTheDocument();
  });

  it("locks the confirmation while the write is in flight", async () => {
    let release!: () => void;
    const onDelete = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    renderTable([runner()], { onDelete });
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" })).toBeDisabled()
    );
    const cancel = within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" });
    expect(cancel).toHaveAttribute("aria-disabled", "true");
    await userEvent.click(cancel);
    expect(screen.getByRole("dialog", { name: "Delete runner?" })).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    release();
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("cancels an idle confirmation without calling the writer", async () => {
    const { onDelete } = renderTable([runner()]);
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(onDelete).not.toHaveBeenCalled();
  });
});
