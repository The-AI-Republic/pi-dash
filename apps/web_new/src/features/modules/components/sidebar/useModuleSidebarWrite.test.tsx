// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ updateModule: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return { ...original, updateModule: mocks.updateModule };
});

vi.mock("../../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError, notifySuccess } from "../../../../shared/shell/toaster.js";
import { moduleKeys } from "../../api/keys.js";
import { MODULE_SAVE_FAILED_BODY } from "./moduleSidebarModel.js";
import { moduleSaveFailureBody, useModuleSidebarWrite } from "./useModuleSidebarWrite.js";

const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";

function setup(
  update: (client: never, slug: string, projectId: string, moduleId: string, patch: unknown) => Promise<unknown>
) {
  const clientObj = {} as never;
  mocks.updateModule.mockImplementation(update);
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  queryClient.setQueryData(moduleKeys.detail("acme", PROJECT, MODULE), {
    id: MODULE,
    name: "Delivery",
    status: "backlog",
  });
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(
    () => useModuleSidebarWrite({ client: clientObj, workspaceSlug: "acme", projectId: PROJECT, moduleId: MODULE }),
    { wrapper }
  );
  return { result, queryClient, clientObj, update };
}

describe("useModuleSidebarWrite", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("paints the patch, writes it, and confirms with the pinned notice", async () => {
    const calls: unknown[] = [];
    const { result, queryClient, clientObj } = setup(async (_client, slug, projectId, moduleId, patch) => {
      calls.push({ slug, projectId, moduleId, patch });
      return {};
    });
    await act(async () => {
      result.current.mutate({ name: "Launch" });
    });
    await waitFor(() =>
      expect(calls).toEqual([{ slug: "acme", projectId: PROJECT, moduleId: MODULE, patch: { name: "Launch" } }])
    );
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module updated successfully."));
    expect(queryClient.getQueryData(moduleKeys.detail("acme", PROJECT, MODULE))).toMatchObject({ name: "Launch" });
    expect(clientObj).toBeDefined();
  });

  it("rolls the detail back and reports the server sentence on refusal", async () => {
    const { result, queryClient } = setup(async () => {
      throw new ApiError({ code: "http_error", message: "The module is archived.", status: 400 });
    });
    act(() => {
      queryClient.setQueryData(moduleKeys.detail("acme", PROJECT, MODULE), { id: MODULE, name: "Delivery" });
    });
    await act(async () => {
      result.current.mutate({ name: "Launch" });
    });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't update the module", "The module is archived.")
    );
    expect(queryClient.getQueryData(moduleKeys.detail("acme", PROJECT, MODULE))).toMatchObject({ name: "Delivery" });
  });

  it("falls back when a refusal has no message", () => {
    expect(moduleSaveFailureBody(new Error(""))).toBe(MODULE_SAVE_FAILED_BODY);
    expect(moduleSaveFailureBody(new ApiError({ code: "http_error", message: "", status: 500 }))).toBe(
      MODULE_SAVE_FAILED_BODY
    );
    expect(moduleSaveFailureBody(new ApiError({ code: "http_error", message: "Specific refusal.", status: 400 }))).toBe(
      "Specific refusal."
    );
  });
});
