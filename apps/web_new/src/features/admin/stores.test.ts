// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { adminKeys } from "./queries.js";
import { registerAdminStoreReset, resetAdminClientState, resetAdminStores, useFirstWorkspaceStore } from "./stores.js";

beforeEach(() => {
  resetAdminStores();
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
