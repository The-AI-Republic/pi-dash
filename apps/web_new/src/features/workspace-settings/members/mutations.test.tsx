// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Member-page writes (WSET-009/010/012/013, spec §10–§13): each hook
// calls its contract with the row identifiers, refreshes both lists when
// it settles, and speaks the pinned notices — success only where the
// spec asks for one, the server's reason otherwise.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contracts, toast } = vi.hoisted(() => ({
  contracts: {
    leaveWorkspace: vi.fn(),
    removeWorkspaceMember: vi.fn(),
    sendWorkspaceInvitations: vi.fn(),
    updateWorkspaceInviteRole: vi.fn(),
    updateWorkspaceMemberRole: vi.fn(),
    withdrawWorkspaceInvite: vi.fn(),
  },
  toast: { notifyError: vi.fn(), notifySuccess: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: toast.notifyError,
  notifySuccess: toast.notifySuccess,
  resolveFileUrl: vi.fn(),
}));

import { rosterKeys } from "./api.js";
import {
  failureMessage,
  INVITATIONS_SENT_MESSAGE,
  INVITATIONS_SENT_TITLE,
  INVITATION_REMOVED_MESSAGE,
  INVITATION_REMOVED_TITLE,
  INVITE_FAILED_MESSAGE,
  LEAVE_FAILED_MESSAGE,
  MEMBER_REMOVE_FAILED_MESSAGE,
  MEMBER_ROLE_FAILED_MESSAGE,
  useChangeInvitationRole,
  useChangeMemberRole,
  useLeaveWorkspace,
  useRemoveMember,
  useSendInvitations,
  useWithdrawInvitation,
  WRITE_FAILED_TITLE,
} from "./mutations.js";

const SLUG = "acme";

function setup<T>(hook: () => T) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const remove = vi.spyOn(queryClient, "removeQueries");
  createAppClient("https://api.test", (async () => {
    throw new Error("no transport expected");
  }) as never);
  const wrapper = ({ children }: { children: React.ReactNode }) =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const view = renderHook(hook, { wrapper });
  return { ...view, invalidate, remove, queryClient };
}

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  resetAppClient();
});

describe("failure message", () => {
  it("prefers the failure's own reason and falls back when it has none", () => {
    expect(failureMessage(new Error("The required object does not exist"), "fallback")).toBe(
      "The required object does not exist"
    );
    expect(failureMessage(new Error("   "), "fallback")).toBe("fallback");
    expect(failureMessage("boom", "fallback")).toBe("fallback");
    expect(failureMessage(null, "fallback")).toBe("fallback");
  });
});

describe("useSendInvitations", () => {
  it("posts every row and toasts the pinned success before refreshing both lists", async () => {
    contracts.sendWorkspaceInvitations.mockResolvedValue({ message: "ok" });
    const { result, invalidate } = setup(() => useSendInvitations(SLUG));
    await act(() =>
      result.current.mutateAsync({
        emails: [
          { email: "ada@example.com", role: 5 },
          { email: "grace@example.com", role: 20 },
        ],
      })
    );
    expect(contracts.sendWorkspaceInvitations).toHaveBeenCalledWith(expect.anything(), SLUG, {
      emails: [
        { email: "ada@example.com", role: 5 },
        { email: "grace@example.com", role: 20 },
      ],
    });
    expect(toast.notifySuccess).toHaveBeenCalledWith(INVITATIONS_SENT_TITLE, INVITATIONS_SENT_MESSAGE);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: rosterKeys.roster(SLUG) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: rosterKeys.invitations(SLUG) });
  });

  it("keeps the dialog's input by failing the mutation and toasts the server reason", async () => {
    contracts.sendWorkspaceInvitations.mockRejectedValue(new Error("A member with this email already exists."));
    const { result, invalidate } = setup(() => useSendInvitations(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync({ emails: [{ email: "ada@example.com", role: 15 }] })).rejects.toThrow(
        "already exists"
      );
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, "A member with this email already exists.");
    expect(toast.notifySuccess).not.toHaveBeenCalled();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: rosterKeys.invitations(SLUG) });
  });

  it("uses the generic failure when the rejection carries no message", async () => {
    contracts.sendWorkspaceInvitations.mockRejectedValue(new Error(""));
    const { result } = setup(() => useSendInvitations(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync({ emails: [] })).rejects.toThrow();
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, INVITE_FAILED_MESSAGE);
  });
});

describe("useChangeInvitationRole", () => {
  it("patches the invitation quietly and refreshes", async () => {
    contracts.updateWorkspaceInviteRole.mockResolvedValue({ id: "inv" });
    const { result, invalidate } = setup(() => useChangeInvitationRole(SLUG));
    await act(() => result.current.mutateAsync({ invitationId: "inv", role: 20 }));
    expect(contracts.updateWorkspaceInviteRole).toHaveBeenCalledWith(expect.anything(), SLUG, "inv", { role: 20 });
    expect(toast.notifySuccess).not.toHaveBeenCalled();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: rosterKeys.invitations(SLUG) });
  });

  it("toasts a refused change and keeps the row on screen with its previous role", async () => {
    contracts.updateWorkspaceInviteRole.mockRejectedValue(new Error("gone"));
    const { result, invalidate } = setup(() => useChangeInvitationRole(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync({ invitationId: "inv", role: 5 })).rejects.toThrow("gone");
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, "gone");
    // §11: a refused write never re-reads, so the row cannot vanish when
    // the server already dropped it.
    expect(invalidate).not.toHaveBeenCalled();
  });
});

describe("useWithdrawInvitation", () => {
  it("deletes the invitation and reports the removal", async () => {
    contracts.withdrawWorkspaceInvite.mockResolvedValue(204);
    const { result, invalidate } = setup(() => useWithdrawInvitation(SLUG));
    await act(() => result.current.mutateAsync({ invitationId: "inv" }));
    expect(contracts.withdrawWorkspaceInvite).toHaveBeenCalledWith(expect.anything(), SLUG, "inv");
    expect(toast.notifySuccess).toHaveBeenCalledWith(INVITATION_REMOVED_TITLE, INVITATION_REMOVED_MESSAGE);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: rosterKeys.invitations(SLUG) });
  });

  it("toasts a failed withdrawal and leaves the visible row untouched", async () => {
    contracts.withdrawWorkspaceInvite.mockRejectedValue(new Error("not found"));
    const { result, invalidate } = setup(() => useWithdrawInvitation(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync({ invitationId: "inv" })).rejects.toThrow("not found");
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, "not found");
    expect(toast.notifySuccess).not.toHaveBeenCalled();
    // §11: the row stays visible after a refused withdrawal.
    expect(invalidate).not.toHaveBeenCalled();
  });
});

describe("useChangeMemberRole", () => {
  it("patches the membership by its id and stays quiet on success", async () => {
    contracts.updateWorkspaceMemberRole.mockResolvedValue({ id: "m1" });
    const { result, invalidate } = setup(() => useChangeMemberRole(SLUG));
    await act(() => result.current.mutateAsync({ membershipId: "m1", role: 20 }));
    expect(contracts.updateWorkspaceMemberRole).toHaveBeenCalledWith(expect.anything(), SLUG, "m1", { role: 20 });
    expect(toast.notifySuccess).not.toHaveBeenCalled();
    expect(toast.notifyError).not.toHaveBeenCalled();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: rosterKeys.roster(SLUG) });
  });

  it("toasts a refused change and keeps the row showing its previous role", async () => {
    contracts.updateWorkspaceMemberRole.mockRejectedValue(new Error("You cannot change your own role"));
    const { result, invalidate } = setup(() => useChangeMemberRole(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync({ membershipId: "m1", role: 5 })).rejects.toThrow("own role");
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, "You cannot change your own role");
    // §12: no refresh on failure, so the row keeps its previous role.
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("falls back to its own wording when the refusal has no reason", async () => {
    contracts.updateWorkspaceMemberRole.mockRejectedValue(new Error(""));
    const { result } = setup(() => useChangeMemberRole(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync({ membershipId: "m1", role: 5 })).rejects.toThrow();
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, MEMBER_ROLE_FAILED_MESSAGE);
  });
});

describe("useRemoveMember", () => {
  it("deletes the membership without a success notice", async () => {
    contracts.removeWorkspaceMember.mockResolvedValue(204);
    const { result, invalidate } = setup(() => useRemoveMember(SLUG));
    await act(() => result.current.mutateAsync({ membershipId: "m1" }));
    expect(contracts.removeWorkspaceMember).toHaveBeenCalledWith(expect.anything(), SLUG, "m1");
    expect(toast.notifySuccess).not.toHaveBeenCalled();
    expect(invalidate).toHaveBeenCalledWith({ queryKey: rosterKeys.roster(SLUG) });
  });

  it("carries the server's reason for a failed removal and keeps the row", async () => {
    contracts.removeWorkspaceMember.mockRejectedValue(new Error("The required object does not exist."));
    const { result, invalidate } = setup(() => useRemoveMember(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync({ membershipId: "m1" })).rejects.toThrow("does not exist");
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, "The required object does not exist.");
    // §13: a refused removal leaves the row active and visible.
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("falls back to its own wording when the refusal has no reason", async () => {
    contracts.removeWorkspaceMember.mockRejectedValue(new Error(" "));
    const { result } = setup(() => useRemoveMember(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync({ membershipId: "m1" })).rejects.toThrow();
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, MEMBER_REMOVE_FAILED_MESSAGE);
  });
});

describe("useLeaveWorkspace", () => {
  it("leaves, drops the workspace subtree, and keeps the session's workspace list stale", async () => {
    contracts.leaveWorkspace.mockResolvedValue(204);
    const { result, remove, invalidate } = setup(() => useLeaveWorkspace(SLUG));
    await act(() => result.current.mutateAsync());
    expect(contracts.leaveWorkspace).toHaveBeenCalledWith(expect.anything(), SLUG);
    await waitFor(() => expect(remove).toHaveBeenCalledWith({ queryKey: ["ws", SLUG] }));
    expect(invalidate).not.toHaveBeenCalled();
    expect(toast.notifySuccess).not.toHaveBeenCalled();
  });

  it("toasts a failed leave", async () => {
    contracts.leaveWorkspace.mockRejectedValue(new Error("The only admin cannot leave the workspace."));
    const { result, remove } = setup(() => useLeaveWorkspace(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync()).rejects.toThrow("only admin");
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, "The only admin cannot leave the workspace.");
    expect(remove).not.toHaveBeenCalled();
  });

  it("falls back to its own wording when the refusal has no reason", async () => {
    contracts.leaveWorkspace.mockRejectedValue(new Error(""));
    const { result } = setup(() => useLeaveWorkspace(SLUG));
    await act(async () => {
      await expect(result.current.mutateAsync()).rejects.toThrow();
    });
    expect(toast.notifyError).toHaveBeenCalledWith(WRITE_FAILED_TITLE, LEAVE_FAILED_MESSAGE);
  });
});
