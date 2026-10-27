// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Refused removal notice (parity/specs/project-settings-members.md
// §"Remove and leave-self"; PSET-024): a DELETE the server refuses
// leaves a durable error toast carrying the server message once the
// confirm dialog closes, with the member table intact (NEWFRONT-398:
// the old notice vanished with the dialog, a flash on the way down).
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ProjectMemberList, TransportResponse, WorkspaceMemberList } from "@pidash/api-client";
import { ToastHost } from "@pidash/kit";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const routerMocks = vi.hoisted(() => ({ navigate: vi.fn(), pathname: "/acme/settings/projects/p/members/" }));

vi.mock("@tanstack/react-router", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useNavigate: () => routerMocks.navigate,
  useRouterState: ({ select }: { select: (state: { location: { pathname: string } }) => unknown }) =>
    select({ location: { pathname: routerMocks.pathname } }),
}));

const readMocks = vi.hoisted(() => ({
  editor: {
    userId: "123e4567-e89b-12d3-a456-42661417400a",
    workspaceRole: 20,
    projectRole: 20,
    ready: true,
  },
}));

vi.mock("./api.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api.js")>();
  const idle = () => ({ mutate: vi.fn(), isPending: false });
  return {
    ...actual,
    useProjectMemberships: vi.fn(),
    useWorkspaceDirectory: vi.fn(),
    useAddProjectMembers: idle,
    useUpdateProjectMemberRole: idle,
    useLeaveProject: idle,
    useSaveProjectDefaults: idle,
    useMemberEditor: () => readMocks.editor,
    useProjectDefaults: () => ({
      data: undefined,
      isPending: true,
      isSuccess: false,
      isError: false,
      error: null,
    }),
  };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { getToastManager } from "../../../shared/shell/index.js";
import { REMOVE_MEMBER_FAILED_TITLE, useProjectMemberships, useWorkspaceDirectory } from "./api.js";
import { clearMemberFilters } from "./filters.js";
import { MembersSettings } from "./MembersSettings.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const ADA = "123e4567-e89b-12d3-a456-42661417400a";
const GRACE = "123e4567-e89b-12d3-a456-42661417400b";

/** The server's answer for a membership deleted out of band. */
const GONE_DETAIL = "Membership is gone.";

const MEMBERSHIPS: ProjectMemberList = [
  { id: "m-ada", role: 20, member: ADA, project: PROJECT, original_role: 20, created_at: "2025-01-10T09:00:00Z" },
  { id: "m-grace", role: 15, member: GRACE, project: PROJECT, original_role: 15, created_at: "2025-03-02T09:00:00Z" },
];

const DIRECTORY: WorkspaceMemberList = [
  {
    id: `${ADA}-membership`,
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
    id: `${GRACE}-membership`,
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

function jsonReply(url: string, status: number, body: unknown): TransportResponse {
  return {
    status,
    url,
    headers: { get: () => null },
    text: async () => JSON.stringify(body),
  };
}

function renderSection(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <MembersSettings workspaceSlug={SLUG} projectId={PROJECT} resolveAvatar={(path) => path} />
      <ToastHost manager={getToastManager()} />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  clearMemberFilters();
  vi.mocked(useProjectMemberships).mockReturnValue({
    data: MEMBERSHIPS,
    isPending: false,
    isSuccess: true,
    isError: false,
    error: null,
  } as never);
  vi.mocked(useWorkspaceDirectory).mockReturnValue({
    data: DIRECTORY,
    isPending: false,
    isSuccess: true,
    isError: false,
    error: null,
  } as never);
});

afterEach(() => {
  clearMemberFilters();
  resetAppClient();
});

describe("MembersSettings refused removal", () => {
  it("keeps the server-message toast up after the confirm dialog closes, with the table intact", async () => {
    const deletes: string[] = [];
    createAppClient("https://api.test", (async (url: string) => {
      if (url.includes("/auth/get-csrf-token/")) return jsonReply(url, 200, { csrf_token: "token" });
      deletes.push(url);
      return jsonReply(url, 404, { detail: GONE_DETAIL });
    }) as never);
    const user = userEvent.setup();
    renderSection();
    const graceRow = (await screen.findAllByTestId("member-roster-row"))[1]!;
    await user.click(within(graceRow).getByRole("button", { name: "Actions for Grace Hopper" }));
    await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
    // The toast also reads as a dialog to the role query, so the
    // confirmation is tracked by its own accessible name throughout.
    const confirmName = "Remove Grace Hopper?";
    await user.click(
      within(await screen.findByRole("dialog", { name: confirmName })).getByRole("button", {
        name: "Remove",
      })
    );

    // The write settles refused: the confirmation withdraws, the row stays,
    // and the notice carrying the server message is still on screen after.
    await waitFor(() => expect(screen.queryByRole("dialog", { name: confirmName })).not.toBeInTheDocument());
    expect(deletes).toEqual([`https://api.test/api/workspaces/${SLUG}/projects/${PROJECT}/members/m-grace/`]);
    const toast = await screen.findByTestId("toast-notification");
    expect(within(toast).getByText(REMOVE_MEMBER_FAILED_TITLE)).toBeInTheDocument();
    expect(within(toast).getByText(GONE_DETAIL)).toBeInTheDocument();
    expect(screen.getAllByTestId("member-roster-row").map((row) => row.getAttribute("data-user-id"))).toEqual([
      ADA,
      GRACE,
    ]);
  });
});
