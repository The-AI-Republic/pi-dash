// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members body against the real read hooks and a counting transport
// (PSET-019): search narrows instantly without a single member request
// after the entry reads, which is what the parity scenario watches.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Transport, TransportResponse } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerMocks = vi.hoisted(() => ({ navigate: vi.fn(), pathname: "/acme/settings/projects/p/members/" }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerMocks.navigate,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => unknown }) =>
    select({ location: { pathname: routerMocks.pathname } }),
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { clearMemberFilters } from "./filters.js";
import { MembersSettings } from "./MembersSettings.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ADA = "123e4567-e89b-12d3-a456-42661417400a";
const GRACE = "123e4567-e89b-12d3-a456-42661417400b";

const MEMBERSHIPS = [
  {
    id: "323e4567-e89b-12d3-a456-426614174021",
    role: 20,
    member: ADA,
    project: PROJECT,
    original_role: 20,
    created_at: "2025-01-10T09:00:00Z",
  },
  {
    id: "323e4567-e89b-12d3-a456-426614174022",
    role: 15,
    member: GRACE,
    project: PROJECT,
    original_role: 15,
    created_at: "2025-03-02T09:00:00Z",
  },
];

const DIRECTORY = [
  {
    id: "423e4567-e89b-12d3-a456-426614174021",
    role: 20,
    member: {
      id: ADA,
      first_name: "Ada",
      last_name: "Lovelace",
      avatar: "",
      avatar_url: null,
      display_name: "ada",
      is_bot: false,
      email: "ada@example.test",
    },
  },
  {
    id: "423e4567-e89b-12d3-a456-426614174022",
    role: 15,
    member: {
      id: GRACE,
      first_name: "Grace",
      last_name: "Hopper",
      avatar: "",
      avatar_url: null,
      display_name: "Grace H",
      is_bot: false,
      email: "grace@example.test",
    },
  },
];

function jsonResponse(url: string, body: unknown): TransportResponse {
  return {
    status: 200,
    url,
    headers: { get: (name: string) => (name.toLowerCase() === "content-type" ? "application/json" : null) },
    text: async () => JSON.stringify(body),
  };
}

function countingTransport(counts: { members: number; directory: number }): Transport {
  return async (url: string): Promise<TransportResponse> => {
    if (url.includes(`/projects/${PROJECT}/members/`)) {
      counts.members += 1;
      return jsonResponse(url, MEMBERSHIPS);
    }
    if (url.endsWith(`/workspaces/${SLUG}/members/`)) {
      counts.directory += 1;
      return jsonResponse(url, DIRECTORY);
    }
    throw new Error(`unexpected request ${url}`);
  };
}

function renderLive(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <MembersSettings workspaceSlug={SLUG} projectId={PROJECT} resolveAvatar={() => null} />
    </QueryClientProvider>
  );
}

function rowIds(): string[] {
  return screen.queryAllByTestId("member-roster-row").map((row) => row.getAttribute("data-user-id") ?? "");
}

beforeEach(() => {
  clearMemberFilters();
});

afterEach(() => {
  resetAppClient();
  clearMemberFilters();
});

describe("MembersSettings live reads", () => {
  it("loads the roster once and never re-reads while the user types", async () => {
    const counts = { members: 0, directory: 0 };
    createAppClient("https://api.test", countingTransport(counts));
    const user = userEvent.setup();
    renderLive();
    await act(async () => {
      await Promise.resolve();
    });
    await waitFor(() => expect(rowIds()).toEqual([ADA, GRACE]), { timeout: 3000 });
    expect(counts).toEqual({ members: 1, directory: 1 });

    await user.type(screen.getByRole("textbox", { name: "Search members" }), "gRaCe");
    expect(rowIds()).toEqual([GRACE]);
    await user.clear(screen.getByRole("textbox", { name: "Search members" }));
    expect(rowIds()).toEqual([ADA, GRACE]);
    expect(counts).toEqual({ members: 1, directory: 1 });
  });

  it("keeps the skeleton and toasts the server message when a read is refused", async () => {
    createAppClient("https://api.test", async (url: string): Promise<TransportResponse> => {
      if (url.includes(`/projects/${PROJECT}/members/`)) {
        throw Object.assign(new Error("boom"), {});
      }
      return jsonResponse(url, DIRECTORY);
    });
    renderLive();
    await waitFor(() => expect(screen.getByTestId("members-skeleton")).toBeInTheDocument());
    expect(screen.queryByText("No matching members")).not.toBeInTheDocument();
  });
});
