// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { DevMachine } from "@pidash/api-client";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const notifyError = vi.fn();
vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: (title: string, description?: string) => notifyError(title, description),
  useDocumentHead: vi.fn(),
}));

import {
  DEV_MACHINES_PAGE_TITLE,
  MACHINE_ACTION_FAILED_BODY,
  MACHINE_ACTION_FAILED_TITLE,
  DevMachinesScreen,
  devMachinesDocumentTitle,
} from "./DevMachinesScreen.js";

function machine(id: string, overrides: Partial<DevMachine> = {}): DevMachine {
  return {
    id,
    host_label: "",
    label: `machine ${id}`,
    visibility: 0,
    runner_count: 0,
    online_runner_count: 0,
    control_online: false,
    last_seen_at: null,
    last_heartbeat_at: null,
    revoked_at: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function setup(overrides: Partial<React.ComponentProps<typeof DevMachinesScreen>> = {}) {
  const handlers = {
    onRotate: vi.fn(async () => undefined),
    onRevoke: vi.fn(async () => undefined),
    onDelete: vi.fn(async () => undefined),
  };
  const view = render(
    <DevMachinesScreen
      workspaceSlug="acme"
      workspaceName="Acme"
      workspaceId="ws-id"
      machines={[machine("one")]}
      loading={false}
      failed={false}
      {...handlers}
      {...overrides}
    />
  );
  return { ...handlers, ...view };
}

afterEach(() => {
  notifyError.mockReset();
});

describe("devMachinesDocumentTitle", () => {
  it("prefixes the workspace name", () => {
    expect(devMachinesDocumentTitle("Acme")).toBe("Acme - AI Dev Machines");
    expect(devMachinesDocumentTitle(undefined)).toBe(DEV_MACHINES_PAGE_TITLE);
  });
});

describe("DevMachinesScreen", () => {
  it("renders the heading, the slot controls and the table", () => {
    setup({ actions: <button type="button">Add runner</button> });
    expect(screen.getByRole("heading", { name: DEV_MACHINES_PAGE_TITLE })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add runner" })).toBeInTheDocument();
    expect(screen.getByText("machine one")).toBeInTheDocument();
  });

  it("runs the requested action and closes the dialog on success", async () => {
    const { onRotate } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Rotate" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Rotate" }));
    await waitFor(() => expect(onRotate).toHaveBeenCalledWith("one"));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  });

  it("deletes with the machine id and keeps no dialog afterward", async () => {
    const { onDelete } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(onDelete).toHaveBeenCalledWith("one"));
  });

  it("keeps the dialog open and toasts when the write fails", async () => {
    const onRevoke = vi.fn(async () => {
      throw new Error("nope");
    });
    setup({ onRevoke });
    fireEvent.click(screen.getByRole("button", { name: "Revoke" }));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke" }));
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith(MACHINE_ACTION_FAILED_TITLE, MACHINE_ACTION_FAILED_BODY.revoke)
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("renders without a workspace id and without pending work", () => {
    setup({ workspaceId: undefined, machines: undefined, loading: true });
    expect(screen.getByRole("heading", { name: DEV_MACHINES_PAGE_TITLE })).toBeInTheDocument();
    expect(screen.getByText("Loading dev machines…")).toBeInTheDocument();
  });

  it("cancels the pending action without writing", async () => {
    const { onDelete } = setup();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
    await act(async () => undefined);
    expect(onDelete).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
