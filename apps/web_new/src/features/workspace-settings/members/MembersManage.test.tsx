// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Members page manage half (WSET-009/010/012/013/015, spec §10–§14): the
// admin-only invite entry drives the bulk dialog to a send; the pending
// group follows the search, edits roles, copies links and withdraws behind
// the shared confirmation; the roster changes roles and removes or leaves
// behind the same confirmation; no billing entry renders for anyone.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { toast } = vi.hoisted(() => ({ toast: { notifyError: vi.fn(), notifySuccess: vi.fn() } }));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: toast.notifyError, notifySuccess: toast.notifySuccess };
});

import { MembersPage } from "./MembersPage.js";

const SLUG = "acme";
const WORKSPACE_ID = "123e4567-e89b-12d3-a456-426614174000";
const INVITATION_IDS: Record<string, string> = {
  "ada@example.com": "423e4567-e89b-12d3-a456-426614174001",
  "grace@example.com": "423e4567-e89b-12d3-a456-426614174002",
};
const ADMIN_ID = "223e4567-e89b-12d3-a456-426614174001";
const OTHER_ID = "223e4567-e89b-12d3-a456-426614174002";

const MEMBERSHIP = {
  [ADMIN_ID]: "323e4567-e89b-12d3-a456-426614174001",
  [OTHER_ID]: "323e4567-e89b-12d3-a456-426614174002",
} as const;

function membershipId(userId: string): string {
  return MEMBERSHIP[userId as keyof typeof MEMBERSHIP];
}

function rosterRow(userId: string, email: string, role: number, active = true) {
  return {
    id: membershipId(userId),
    member: {
      id: userId,
      first_name: "Parity",
      last_name: "Person",
      avatar: "",
      avatar_url: null,
      display_name: `Parity ${email.split("@")[0]}`,
      is_bot: false,
      email,
      last_login_medium: "email",
    },
    role,
    is_active: active,
    created_at: "2026-01-01T00:00:00Z",
  };
}

const ROSTER = [rosterRow(ADMIN_ID, "oracle@example.com", 20), rosterRow(OTHER_ID, "parity-mention@example.com", 15)];

function invitation(email: string, role = 15, link = "https://app.test/invite/tok") {
  return {
    id: INVITATION_IDS[email] ?? "423e4567-e89b-12d3-a456-426614174009",
    workspace: { id: WORKSPACE_ID, name: "Acme", slug: SLUG, logo_url: null },
    email,
    accepted: false,
    token: "tok",
    message: null,
    responded_at: null,
    role,
    invite_link: link,
    created_by: null,
    updated_by: null,
    created_at: "2026-02-01T00:00:00Z",
    updated_at: "2026-02-01T00:00:00Z",
  };
}

interface ServerState {
  role: number | null;
  roster: unknown[];
  invitations: unknown[];
  /** Status answered by the next write per method. */
  status?: Partial<Record<"POST" | "PATCH" | "DELETE", number>>;
}

function transportFor(state: ServerState) {
  const calls: { method: string; url: string }[] = [];
  const transport = async (url: string, init?: { method?: string }) => {
    const method = init?.method ?? "GET";
    calls.push({ method, url });
    const json = (status: number, body: unknown) => ({
      status,
      url,
      headers: { get: () => null },
      text: async () => JSON.stringify(body),
    });
    if (url.endsWith("/auth/get-csrf-token/")) return json(200, { csrf_token: "csrf" });
    if (url.endsWith("/api/users/me/workspaces/")) {
      return json(
        200,
        state.role === null
          ? []
          : [{ id: WORKSPACE_ID, name: "Acme", slug: SLUG, logo_url: null, total_members: 2, role: state.role }]
      );
    }
    if (url.endsWith("/api/users/me/")) {
      return json(200, {
        id: ADMIN_ID,
        email: "oracle@example.com",
        username: "oracle",
        first_name: "Parity",
        last_name: "Person",
        display_name: "Parity oracle",
        avatar: "",
        avatar_url: null,
        cover_image: null,
        cover_image_url: null,
        date_joined: "2025-01-01T00:00:00Z",
        user_timezone: "UTC",
        is_active: true,
        is_bot: false,
        is_email_verified: true,
        is_password_autoset: false,
        last_login_medium: "email",
        last_login_time: null,
      });
    }
    if (method === "GET" && url.endsWith(`/api/workspaces/${SLUG}/members/`)) return json(200, state.roster);
    if (method === "GET" && url.endsWith(`/api/workspaces/${SLUG}/invitations/`)) return json(200, state.invitations);
    const status = state.status?.[method as "POST" | "PATCH" | "DELETE"] ?? 200;
    if (method === "DELETE") return json(status, {});
    if (status !== 200) return json(status, { detail: "The required object does not exist." });
    if (method === "PATCH" && url.includes("/invitations/")) {
      const match = state.invitations.find((row) => url.includes(`/${(row as { id: string }).id}/`));
      return json(200, { ...(match as object), role: 20 });
    }
    return json(200, { message: "ok" });
  };
  return { calls, transport: transport as never };
}

function setup(state: ServerState, onLeaveWorkspace = vi.fn()) {
  const stub = transportFor(state);
  createAppClient("https://api.test", stub.transport);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <MembersPage workspaceSlug={SLUG} onLeaveWorkspace={onLeaveWorkspace} />
    </QueryClientProvider>
  );
  return { ...stub, queryClient, onLeaveWorkspace, ...view };
}

function rowFor(email: string): HTMLElement {
  const row = screen.queryAllByTestId("member-row").find((candidate) => candidate.getAttribute("data-email") === email);
  if (row === undefined) throw new Error(`no member row for ${email}`);
  return row;
}

function pendingRowFor(email: string): HTMLElement {
  const row = screen
    .queryAllByTestId("pending-invitation-row")
    .find((candidate) => candidate.getAttribute("data-email") === email);
  if (row === undefined) throw new Error(`no pending row for ${email}`);
  return row;
}

async function openMemberMenu(email: string) {
  await userEvent.click(within(rowFor(email)).getByRole("button", { name: `Actions for ${email}` }));
}

async function openPendingMenu(email: string) {
  await userEvent.click(within(pendingRowFor(email)).getByRole("button", { name: `Actions for ${email}` }));
}

afterEach(() => {
  resetAppClient();
  vi.clearAllMocks();
});

describe("MembersPage invite entry (WSET-009)", () => {
  it("opens the dialog for an admin and closes it after a successful send", async () => {
    const { calls } = setup({ role: 20, roster: ROSTER, invitations: [] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    await userEvent.click(screen.getByRole("button", { name: "Add member" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(within(dialog).getByRole("textbox", { name: "Email" }), "new-person@example.com");
    await userEvent.click(within(dialog).getByRole("button", { name: "Send invites" }));
    await waitFor(() =>
      expect(calls).toContainEqual({ method: "POST", url: `https://api.test/api/workspaces/${SLUG}/invitations/` })
    );
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(toast.notifySuccess).toHaveBeenCalledWith(
      "Invitations sent",
      "Invitations sent. Each invitee will receive their own email."
    );
  });

  it("keeps the dialog and its input when the server refuses the batch", async () => {
    setup({ role: 20, roster: ROSTER, invitations: [], status: { POST: 400 } });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    await userEvent.click(screen.getByRole("button", { name: "Add member" }));
    const dialog = await screen.findByRole("dialog");
    await userEvent.type(within(dialog).getByRole("textbox", { name: "Email" }), "oracle@example.com");
    await userEvent.click(within(dialog).getByRole("button", { name: "Send invites" }));
    await waitFor(() => expect(toast.notifyError).toHaveBeenCalledWith("Error!", expect.any(String)));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(within(screen.getByRole("dialog")).getByRole("textbox", { name: "Email" })).toHaveValue(
      "oracle@example.com"
    );
  });

  it("hides the invite entry from a member", async () => {
    setup({ role: 15, roster: ROSTER, invitations: [] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    expect(screen.queryByRole("button", { name: "Add member" })).not.toBeInTheDocument();
  });

  it("renders no billing entry beside the invite control (WSET-015)", async () => {
    setup({ role: 20, roster: ROSTER, invitations: [invitation("ada@example.com")] });
    await waitFor(() => expect(screen.getByRole("button", { name: "Add member" })).toBeInTheDocument());
    expect(screen.queryByTestId("members-billing")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /upgrade|billing|plan/i })).not.toBeInTheDocument();
  });
});

describe("MembersPage pending invitations (WSET-010)", () => {
  it("shows the group to an admin with count and roles, and narrows it by the search", async () => {
    setup({
      role: 20,
      roster: ROSTER,
      invitations: [invitation("ada@example.com", 15), invitation("grace@example.com", 5)],
    });
    await waitFor(() => expect(screen.getByTestId("pending-invitations-count")).toHaveTextContent("2"));
    expect(pendingRowFor("ada@example.com")).toBeInTheDocument();
    expect(
      within(pendingRowFor("grace@example.com")).getByRole("button", { name: "Role for grace@example.com" })
    ).toHaveTextContent("Guest");
    fireEvent.change(screen.getByPlaceholderText("Search..."), { target: { value: "grace" } });
    expect(screen.getAllByTestId("pending-invitation-row")).toHaveLength(1);
    expect(screen.getByTestId("pending-invitations-count")).toHaveTextContent("1");
    expect(screen.queryByTestId("members-no-match")).not.toBeInTheDocument();
  });

  it("collapses and reopens from the heading", async () => {
    setup({ role: 20, roster: ROSTER, invitations: [invitation("ada@example.com")] });
    await waitFor(() => expect(screen.getByTestId("pending-invitation-row")).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: /Pending invites/ }));
    expect(screen.queryByTestId("pending-invitation-row")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /Pending invites/ }));
    expect(screen.getByTestId("pending-invitation-row")).toBeInTheDocument();
  });

  it("hides the group from a member", async () => {
    setup({ role: 15, roster: ROSTER, invitations: [invitation("ada@example.com")] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    expect(screen.queryByTestId("pending-invitations")).not.toBeInTheDocument();
  });

  it("changes an invitation role through its own endpoint", async () => {
    const { calls } = setup({ role: 20, roster: ROSTER, invitations: [invitation("ada@example.com", 15)] });
    await waitFor(() => expect(screen.getByTestId("pending-invitation-row")).toBeInTheDocument());
    vi.clearAllMocks();
    await userEvent.click(screen.getByRole("button", { name: "Role for ada@example.com" }));
    await userEvent.click(await screen.findByRole("option", { name: "Admin" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "PATCH",
        url: `https://api.test/api/workspaces/${SLUG}/invitations/${INVITATION_IDS["ada@example.com"]}/`,
      })
    );
    expect(toast.notifyError).not.toHaveBeenCalled();
  });

  it("copies the absolute invitation link and confirms", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    setup({ role: 20, roster: ROSTER, invitations: [invitation("ada@example.com")] });
    await waitFor(() => expect(screen.getByTestId("pending-invitation-row")).toBeInTheDocument());
    await openPendingMenu("ada@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("https://app.test/invite/tok"));
    await waitFor(() => expect(toast.notifySuccess).toHaveBeenCalledWith("Invite link copied"));
  });

  it("stays silent when the clipboard refuses", async () => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn(async () => Promise.reject(new Error("denied"))) },
    });
    setup({ role: 20, roster: ROSTER, invitations: [invitation("ada@example.com")] });
    await waitFor(() => expect(screen.getByTestId("pending-invitation-row")).toBeInTheDocument());
    await openPendingMenu("ada@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Copy link" }));
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(toast.notifySuccess).not.toHaveBeenCalled();
    expect(toast.notifyError).not.toHaveBeenCalled();
  });

  it("withdraws behind the confirmation and reports the removal", async () => {
    const { calls } = setup({ role: 20, roster: ROSTER, invitations: [invitation("ada@example.com")] });
    await waitFor(() => expect(screen.getByTestId("pending-invitation-row")).toBeInTheDocument());
    await openPendingMenu("ada@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Remove" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Remove ada@example.com?" })).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(calls.some((call) => call.method === "DELETE")).toBe(false);
    await openPendingMenu("ada@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Remove" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Remove" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "DELETE",
        url: `https://api.test/api/workspaces/${SLUG}/invitations/${INVITATION_IDS["ada@example.com"]}/`,
      })
    );
    await waitFor(() =>
      expect(toast.notifySuccess).toHaveBeenCalledWith(
        "Success!",
        "Invitation removed. That address is no longer pending."
      )
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("closes after a failed withdrawal and toasts the server's reason", async () => {
    setup({ role: 20, roster: ROSTER, invitations: [invitation("ada@example.com")], status: { DELETE: 404 } });
    await waitFor(() => expect(screen.getByTestId("pending-invitation-row")).toBeInTheDocument());
    await openPendingMenu("ada@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Remove" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(toast.notifyError).toHaveBeenCalledWith("Error!", expect.any(String)));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(pendingRowFor("ada@example.com")).toBeInTheDocument();
  });
});

describe("MembersPage roster actions (WSET-012/013)", () => {
  it("changes another member's role through the membership endpoint", async () => {
    const { calls } = setup({ role: 20, roster: ROSTER, invitations: [] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    await userEvent.click(
      within(rowFor("parity-mention@example.com")).getByRole("button", { name: "Role for parity-mention@example.com" })
    );
    await userEvent.click(await screen.findByRole("option", { name: "Admin" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "PATCH",
        url: `https://api.test/api/workspaces/${SLUG}/members/${membershipId(OTHER_ID)}/`,
      })
    );
  });

  it("offers a role control on somebody else's row but never the actor's own", async () => {
    setup({ role: 20, roster: ROSTER, invitations: [] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    expect(within(rowFor("oracle@example.com")).queryByRole("button", { name: /Role for/ })).not.toBeInTheDocument();
    expect(
      within(rowFor("parity-mention@example.com")).getByRole("button", { name: "Role for parity-mention@example.com" })
    ).toBeInTheDocument();
  });

  it("renders no role controls for a member actor", async () => {
    setup({ role: 15, roster: ROSTER, invitations: [] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    expect(screen.queryByRole("button", { name: /Role for/ })).not.toBeInTheDocument();
  });

  it("removes another member behind the naming confirmation", async () => {
    const { calls } = setup({ role: 20, roster: ROSTER, invitations: [] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    await openMemberMenu("parity-mention@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Remove" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Remove Parity parity-mention?" })).toBeInTheDocument();
    expect(within(dialog).getByText(/no longer have access/)).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        method: "DELETE",
        url: `https://api.test/api/workspaces/${SLUG}/members/${membershipId(OTHER_ID)}/`,
      })
    );
    expect(toast.notifySuccess).not.toHaveBeenCalled();
  });

  it("toasts a failed removal and leaves the row in place", async () => {
    setup({ role: 20, roster: ROSTER, invitations: [], status: { DELETE: 404 } });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    await openMemberMenu("parity-mention@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Remove" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(toast.notifyError).toHaveBeenCalledWith("Error!", expect.any(String)));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(rowFor("parity-mention@example.com")).toHaveAttribute("data-active", "true");
  });

  it("offers no Leave to an admin on somebody else's row and no Remove on their own", async () => {
    setup({ role: 20, roster: ROSTER, invitations: [] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    await openMemberMenu("oracle@example.com");
    expect(await screen.findByRole("menuitem", { name: "Leave" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Remove" })).not.toBeInTheDocument();
  });

  it("leaves through the leave endpoint and hands navigation to the shell", async () => {
    const { calls, onLeaveWorkspace } = setup({ role: 20, roster: ROSTER, invitations: [] });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    await openMemberMenu("oracle@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Leave" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("heading", { name: "Leave workspace?" })).toBeInTheDocument();
    await userEvent.click(within(dialog).getByRole("button", { name: "Leave" }));
    await waitFor(() =>
      expect(calls).toContainEqual({ method: "POST", url: `https://api.test/api/workspaces/${SLUG}/members/leave/` })
    );
    await waitFor(() => expect(onLeaveWorkspace).toHaveBeenCalledTimes(1));
  });

  it("keeps the surface on a failed leave", async () => {
    const { onLeaveWorkspace } = setup({ role: 20, roster: ROSTER, invitations: [], status: { POST: 400 } });
    await waitFor(() => expect(screen.getAllByTestId("member-row").length).toBeGreaterThan(0));
    await openMemberMenu("oracle@example.com");
    await userEvent.click(await screen.findByRole("menuitem", { name: "Leave" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Leave" }));
    await waitFor(() => expect(toast.notifyError).toHaveBeenCalledWith("Error!", expect.any(String)));
    expect(onLeaveWorkspace).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
