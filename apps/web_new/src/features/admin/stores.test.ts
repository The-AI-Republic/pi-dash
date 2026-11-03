// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ADMIN_SIDEBAR_COLLAPSED_KEY } from "./adminShell.js";
import { adminKeys } from "./queries.js";
import {
  registerAdminStoreReset,
  resetAdminClientState,
  resetAdminStores,
  useAdminSidebarStore,
  useFirstWorkspaceStore,
  useWorkspaceCreateDraftStore,
  WORKSPACE_CREATE_BLANK_DRAFT,
} from "./stores.js";

beforeEach(() => {
  window.localStorage.clear();
  resetAdminStores();
});

afterEach(() => {
  window.localStorage.clear();
});

describe("admin reset registry", () => {
  it("runs every registered reset", () => {
    const first = vi.fn();
    const second = vi.fn();
    const releaseFirst = registerAdminStoreReset(first);
    const releaseSecond = registerAdminStoreReset(second);
    try {
      resetAdminStores();
      expect(first).toHaveBeenCalledTimes(1);
      expect(second).toHaveBeenCalledTimes(1);
    } finally {
      releaseFirst();
      releaseSecond();
    }
  });

  it("drops unregistered resets", () => {
    const reset = vi.fn();
    const release = registerAdminStoreReset(reset);
    release();
    resetAdminStores();
    expect(reset).not.toHaveBeenCalled();
  });
});

describe("first-workspace flag", () => {
  it("starts un-dismissed", () => {
    expect(useFirstWorkspaceStore.getState().dismissed).toBe(false);
  });

  it("dismisses and resets", () => {
    useFirstWorkspaceStore.getState().dismiss();
    expect(useFirstWorkspaceStore.getState().dismissed).toBe(true);
    resetAdminStores();
    expect(useFirstWorkspaceStore.getState().dismissed).toBe(false);
  });
});

describe("sidebar collapsed flag", () => {
  it("starts expanded with nothing persisted", () => {
    expect(useAdminSidebarStore.getState().collapsed).toBe(false);
  });

  it("persists through set and toggle", () => {
    useAdminSidebarStore.getState().setCollapsed(true);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("true");
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
    useAdminSidebarStore.getState().toggleCollapsed();
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("false");
    expect(useAdminSidebarStore.getState().collapsed).toBe(false);
    useAdminSidebarStore.getState().toggleCollapsed();
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
  });

  it("restores the persisted flag on reset instead of clearing it", () => {
    useAdminSidebarStore.getState().setCollapsed(true);
    // Drift the live value away from storage, as a narrow viewport
    // could before the sign-out reset runs.
    useAdminSidebarStore.setState({ collapsed: false });
    resetAdminStores();
    expect(useAdminSidebarStore.getState().collapsed).toBe(true);
    expect(window.localStorage.getItem(ADMIN_SIDEBAR_COLLAPSED_KEY)).toBe("true");
  });
});

describe("create-workspace draft", () => {
  it("starts blank", () => {
    expect(WORKSPACE_CREATE_BLANK_DRAFT).toEqual({ name: "", slug: "", organizationSize: "" });
    expect(useWorkspaceCreateDraftStore.getState().name).toBe("");
    expect(useWorkspaceCreateDraftStore.getState().slug).toBe("");
    expect(useWorkspaceCreateDraftStore.getState().organizationSize).toBe("");
  });

  it("merges typed values and clears back to blank", () => {
    useWorkspaceCreateDraftStore.getState().setDraft({ name: "Good Name" });
    useWorkspaceCreateDraftStore.getState().setDraft({ slug: "good-slug", organizationSize: "2-10" });
    expect(useWorkspaceCreateDraftStore.getState().name).toBe("Good Name");
    expect(useWorkspaceCreateDraftStore.getState().slug).toBe("good-slug");
    expect(useWorkspaceCreateDraftStore.getState().organizationSize).toBe("2-10");
    useWorkspaceCreateDraftStore.getState().resetDraft();
    expect(useWorkspaceCreateDraftStore.getState().name).toBe("");
    expect(useWorkspaceCreateDraftStore.getState().slug).toBe("");
    expect(useWorkspaceCreateDraftStore.getState().organizationSize).toBe("");
  });

  it("clears with the sign-out reset", () => {
    useWorkspaceCreateDraftStore.getState().setDraft({ name: "Good Name", slug: "good-slug" });
    resetAdminStores();
    expect(useWorkspaceCreateDraftStore.getState().name).toBe("");
    expect(useWorkspaceCreateDraftStore.getState().slug).toBe("");
  });
});

describe("resetAdminClientState", () => {
  it("clears exactly the admin query subtree", () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(adminKeys.bootstrap, { instance: {} });
    queryClient.setQueryData(adminKeys.session, { kind: "admin" });
    queryClient.setQueryData(["ws", "acme"], { slug: "acme" });
    useFirstWorkspaceStore.getState().dismiss();

    resetAdminClientState(queryClient);

    expect(queryClient.getQueryData(adminKeys.bootstrap)).toBeUndefined();
    expect(queryClient.getQueryData(adminKeys.session)).toBeUndefined();
    expect(queryClient.getQueryData(["ws", "acme"])).toEqual({ slug: "acme" });
    expect(useFirstWorkspaceStore.getState().dismissed).toBe(false);
  });
});
