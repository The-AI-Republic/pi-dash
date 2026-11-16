// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Attach-existing bridge (parity/specs/issues.md §8, ISS-069/070). The
// hook is the one place the group headers and the empty cards hand picks
// to, so its writes, notices and refresh are pinned here.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  addToCycle: vi.fn(),
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
  addIssueToCycle: mocks.addToCycle,
  addIssuesToModule: mocks.addToModule,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { issueKeys } from "../api/keys.js";
import { entityListsKeyPart, type IssuesEntity } from "../prefs/entities.js";
import { ATTACH_FAILED_TITLE, useAttachExisting } from "./attach.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const CYCLE = "423e4567-e89b-12d3-a456-426614174003";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";
const CYCLE_ENTITY: IssuesEntity = { scope: "cycle", projectId: PROJECT, cycleId: CYCLE };
const MODULE_ENTITY: IssuesEntity = { scope: "module", projectId: PROJECT, moduleId: MODULE };

function wrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: React.ReactNode }): React.ReactElement {
    return React.createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

function mount(entity: IssuesEntity, extra: { cycleId?: string; moduleId?: string } = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries");
  const view = renderHook(() => useAttachExisting({ workspaceSlug: SLUG, projectId: PROJECT, entity, ...extra }), {
    wrapper: wrapper(queryClient),
  });
  return { ...view, invalidate };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.addToCycle.mockResolvedValue(undefined);
  mocks.addToModule.mockResolvedValue(undefined);
  createAppClient("https://api.test", (async () => {
    throw new Error("unexpected network");
  }) as never);
});

afterEach(() => {
  resetAppClient();
});

describe("useAttachExisting", () => {
  it("writes each pick onto the cycle in order and refreshes its lists", async () => {
    const { result, invalidate } = mount(CYCLE_ENTITY, { cycleId: CYCLE });
    let outcome = false;
    await act(async () => {
      outcome = await result.current.attach(["a", "b"]);
    });
    expect(outcome).toBe(true);
    expect(mocks.addToCycle.mock.calls.map((call) => call.slice(2))).toEqual([
      [PROJECT, CYCLE, "a"],
      [PROJECT, CYCLE, "b"],
    ]);
    expect(mocks.addToModule).not.toHaveBeenCalled();
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Work items added");
    expect(invalidate).toHaveBeenCalledWith({
      queryKey: issueKeys.lists(SLUG, ...entityListsKeyPart(CYCLE_ENTITY)),
    });
    expect(result.current.isAttaching).toBe(false);
  });

  it("writes one batch onto the module", async () => {
    const { result } = mount(MODULE_ENTITY, { moduleId: MODULE });
    let outcome = false;
    await act(async () => {
      outcome = await result.current.attach(["a"]);
    });
    expect(outcome).toBe(true);
    expect(mocks.addToModule).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, MODULE, ["a"]);
    expect(mocks.addToCycle).not.toHaveBeenCalled();
    expect(mocks.notifySuccess).toHaveBeenCalledWith("Work item added");
  });

  it("prefers the cycle when a page carries both scopes", async () => {
    const { result } = mount(CYCLE_ENTITY, { cycleId: CYCLE, moduleId: MODULE });
    await act(async () => {
      await result.current.attach(["a"]);
    });
    expect(mocks.addToCycle).toHaveBeenCalledTimes(1);
    expect(mocks.addToModule).not.toHaveBeenCalled();
  });

  it("reports a page with neither scope instead of silently succeeding", async () => {
    const { result, invalidate } = mount({ scope: "project", projectId: PROJECT });
    let outcome = true;
    await act(async () => {
      outcome = await result.current.attach(["a"]);
    });
    expect(outcome).toBe(false);
    expect(mocks.notifyError).toHaveBeenCalledWith(ATTACH_FAILED_TITLE, "Check your connection and try again.");
    expect(mocks.notifySuccess).not.toHaveBeenCalled();
    expect(invalidate).not.toHaveBeenCalled();
    expect(result.current.isAttaching).toBe(false);
  });

  it("reports a failed write and still releases the busy state", async () => {
    mocks.addToModule.mockRejectedValue(new Error("bridge down"));
    const { result, invalidate } = mount(MODULE_ENTITY, { moduleId: MODULE });
    let outcome = true;
    await act(async () => {
      outcome = await result.current.attach(["a"]);
    });
    expect(outcome).toBe(false);
    expect(mocks.notifyError).toHaveBeenCalledWith(ATTACH_FAILED_TITLE, "Check your connection and try again.");
    expect(invalidate).not.toHaveBeenCalled();
    expect(result.current.isAttaching).toBe(false);
  });

  it("marks the hook busy while the write is in flight", async () => {
    let release: () => void = () => {};
    mocks.addToModule.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        })
    );
    const { result } = mount(MODULE_ENTITY, { moduleId: MODULE });
    act(() => {
      void result.current.attach(["a"]);
    });
    await waitFor(() => expect(result.current.isAttaching).toBe(true));
    await act(async () => {
      release();
    });
    await waitFor(() => expect(result.current.isAttaching).toBe(false));
  });
});
