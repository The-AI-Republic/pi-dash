// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import * as React from "react";
import type { Workspace } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const useDevMachines = vi.fn();
const rotateAsync = vi.fn(async () => undefined);
const revokeAsync = vi.fn(async () => undefined);
const deleteAsync = vi.fn(async () => undefined);

vi.mock("../api/devMachines.js", () => ({
  useDevMachines: (slug: string, id: string | undefined) => useDevMachines(slug, id),
  useRotateDevMachine: () => ({ mutateAsync: rotateAsync }),
  useRevokeDevMachine: () => ({ mutateAsync: revokeAsync }),
  useDeleteDevMachine: () => ({ mutateAsync: deleteAsync }),
}));

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { sessionKeys, setSessionClient } = await import("../../../core/session/index.js");
const { DevMachinesContainer } = await import("./DevMachinesContainer.js");

function mount(workspaces: Workspace[] | undefined): void {
  setSessionClient({} as never);
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  if (workspaces !== undefined) queryClient.setQueryData(sessionKeys.workspaces, workspaces);
  render(
    <QueryClientProvider client={queryClient}>
      <DevMachinesContainer workspaceSlug="acme" />
    </QueryClientProvider>
  );
}

const workspace: Workspace = {
  id: "ws-id",
  name: "Acme",
  slug: "acme",
  logo_url: null,
  total_members: 1,
  role: 15,
};

beforeEach(() => {
  vi.clearAllMocks();
  useDevMachines.mockReturnValue({ data: [], isPending: false, isError: false });
  rotateAsync.mockResolvedValue(undefined);
  revokeAsync.mockResolvedValue(undefined);
  deleteAsync.mockResolvedValue(undefined);
});

afterEach(() => {
  resetAppClient();
});

describe("DevMachinesContainer", () => {
  it("renders nothing until the workspace row resolves", () => {
    mount(undefined);
    expect(screen.queryByRole("heading", { name: "AI Dev Machines" })).not.toBeInTheDocument();
  });

  it("passes the workspace id into the machines read", () => {
    mount([workspace]);
    expect(useDevMachines).toHaveBeenCalledWith("acme", "ws-id");
    expect(screen.getByRole("heading", { name: "AI Dev Machines" })).toBeInTheDocument();
  });

  it("wires the row writers to the machine mutations", async () => {
    useDevMachines.mockReturnValue({
      data: [
        {
          id: "12345678-aaaa-bbbb-cccc-dddddddddddd",
          host_label: "",
          label: "build box",
          visibility: 0,
          runner_count: 0,
          online_runner_count: 0,
          control_online: false,
          last_seen_at: null,
          last_heartbeat_at: null,
          revoked_at: null,
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
        },
      ],
      isPending: false,
      isError: false,
    });
    mount([workspace]);
    for (const [label, writer] of [
      ["Rotate", rotateAsync],
      ["Revoke", revokeAsync],
      ["Delete", deleteAsync],
    ] as const) {
      fireEvent.click(screen.getByRole("button", { name: label }));
      const dialog = screen.getByRole("dialog");
      fireEvent.click(within(dialog).getByRole("button", { name: label }));
      await waitFor(() => expect(writer).toHaveBeenCalledWith("12345678-aaaa-bbbb-cccc-dddddddddddd"));
    }
  });

  it("surfaces the read's progress and failure", () => {
    useDevMachines.mockReturnValue({ data: undefined, isPending: true, isError: false });
    mount([workspace]);
    expect(screen.getByText("Loading dev machines…")).toBeInTheDocument();
  });
});
