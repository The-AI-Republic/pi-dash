// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The module's add-existing write (parity/specs/modules.md §8.3, MOD-031):
// one POST writes every pick, the pinned notice follows, and the module's
// work-item lists and its detail read (the badge) refresh. A refusal
// notices the pinned failure and leaves the queries alone.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  addToModule: vi.fn(),
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: mocks.notifySuccess,
  notifyError: mocks.notifyError,
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  addIssuesToModule: mocks.addToModule,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { issueKeys } from "../../issues/api/keys.js";
import { entityListsKeyPart } from "../../issues/prefs/entities.js";
import { moduleKeys } from "../api/keys.js";
import { MODULE_ADD_EXISTING_FAILURE, MODULE_ADD_EXISTING_SUCCESS } from "./moduleAddWorkItem.js";
import { useModuleAddExisting } from "./useModuleAddExisting.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";

function mount() {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue(undefined);
  const wrapper = ({ children }: { children: React.ReactNode }): React.ReactElement =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const view = renderHook(() => useModuleAddExisting({ workspaceSlug: SLUG, projectId: PROJECT, moduleId: MODULE }), {
    wrapper,
  });
  return { ...view, invalidate };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.addToModule.mockResolvedValue({ message: "ok" });
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useModuleAddExisting", () => {
  it("writes the picks, notices success and refreshes the module reads", async () => {
    const { result, invalidate } = mount();
    let outcome = false;
    await act(async () => {
      outcome = await result.current.addExisting(["issue-a", "issue-b"]);
    });
    expect(outcome).toBe(true);
    expect(mocks.addToModule).toHaveBeenCalledWith({ stub: true }, SLUG, PROJECT, MODULE, ["issue-a", "issue-b"]);
    expect(mocks.notifySuccess).toHaveBeenCalledWith(MODULE_ADD_EXISTING_SUCCESS);
    expect(mocks.notifyError).not.toHaveBeenCalled();
    expect(result.current.isAdding).toBe(false);
    const keys = invalidate.mock.calls.map((call) => (call[0] as { queryKey: unknown }).queryKey);
    expect(keys).toEqual([
      issueKeys.lists(SLUG, ...entityListsKeyPart({ scope: "module", projectId: PROJECT, moduleId: MODULE })),
      moduleKeys.detail(SLUG, PROJECT, MODULE),
    ]);
  });

  it("notices the pinned failure and refreshes nothing when the write is refused", async () => {
    mocks.addToModule.mockRejectedValue(new Error("refused"));
    const { result, invalidate } = mount();
    let outcome = true;
    await act(async () => {
      outcome = await result.current.addExisting(["issue-a"]);
    });
    expect(outcome).toBe(false);
    expect(mocks.notifyError).toHaveBeenCalledWith(MODULE_ADD_EXISTING_FAILURE);
    expect(mocks.notifySuccess).not.toHaveBeenCalled();
    expect(invalidate).not.toHaveBeenCalled();
    expect(result.current.isAdding).toBe(false);
  });

  it("ignores an empty pick without writing or noticing", async () => {
    const { result, invalidate } = mount();
    let outcome = true;
    await act(async () => {
      outcome = await result.current.addExisting([]);
    });
    expect(outcome).toBe(false);
    expect(mocks.addToModule).not.toHaveBeenCalled();
    expect(mocks.notifySuccess).not.toHaveBeenCalled();
    expect(mocks.notifyError).not.toHaveBeenCalled();
    expect(invalidate).not.toHaveBeenCalled();
  });
});
