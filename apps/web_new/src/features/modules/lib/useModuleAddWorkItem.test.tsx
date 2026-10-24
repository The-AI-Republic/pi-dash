// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The page's add controller (parity/specs/modules.md §8.3, MOD-031): one
// creation dialog and one picker per visit, both refused for a standing
// that may not add, and a refusal can never open either through the
// setters the dialogs call on close.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
  notifyError: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  addIssuesToModule: vi.fn(async () => ({ message: "ok" })),
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { moduleKeys } from "../api/keys.js";
import { useModuleAddWorkItem, type UseModuleAddWorkItemInput } from "./useModuleAddWorkItem.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "523e4567-e89b-12d3-a456-426614174004";

function mount(overrides: Partial<UseModuleAddWorkItemInput> = {}) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const invalidate = vi.spyOn(queryClient, "invalidateQueries").mockResolvedValue(undefined);
  const wrapper = ({ children }: { children: React.ReactNode }): React.ReactElement =>
    React.createElement(QueryClientProvider, { client: queryClient }, children);
  const input: UseModuleAddWorkItemInput = {
    workspaceSlug: SLUG,
    projectId: PROJECT,
    moduleId: MODULE,
    memberRole: 20,
    projectArchived: false,
    ...overrides,
  };
  const view = renderHook(() => useModuleAddWorkItem(input), { wrapper });
  return { ...view, invalidate };
}

describe("useModuleAddWorkItem", () => {
  it("opens the creation dialog and the picker for an editing standing", () => {
    const { result } = mount();
    expect(result.current.canAdd).toBe(true);
    expect(result.current.createOpen).toBe(false);
    expect(result.current.pickerOpen).toBe(false);
    act(() => {
      result.current.openCreate();
      result.current.openPicker();
    });
    expect(result.current.createOpen).toBe(true);
    expect(result.current.pickerOpen).toBe(true);
    act(() => {
      result.current.setCreateOpen(false);
      result.current.setPickerOpen(false);
    });
    expect(result.current.createOpen).toBe(false);
    expect(result.current.pickerOpen).toBe(false);
  });

  it("cannot open either flow for a guest or an archived project", () => {
    const guest = mount({ memberRole: 5 });
    act(() => {
      guest.result.current.openCreate();
      guest.result.current.openPicker();
      guest.result.current.setCreateOpen(true);
      guest.result.current.setPickerOpen(true);
    });
    expect(guest.result.current.canAdd).toBe(false);
    expect(guest.result.current.createOpen).toBe(false);
    expect(guest.result.current.pickerOpen).toBe(false);
    const archived = mount({ projectArchived: true });
    act(() => {
      archived.result.current.openCreate();
      archived.result.current.openPicker();
    });
    expect(archived.result.current.canAdd).toBe(false);
    expect(archived.result.current.createOpen).toBe(false);
    expect(archived.result.current.pickerOpen).toBe(false);
  });

  it("refreshes the module detail after a create without closing the form", () => {
    const { result, invalidate } = mount();
    act(() => {
      result.current.openCreate();
    });
    expect(result.current.createOpen).toBe(true);
    act(() => {
      result.current.handleCreated();
    });
    // Create more keeps the dialog mounted until it calls the setter.
    expect(result.current.createOpen).toBe(true);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: moduleKeys.detail(SLUG, PROJECT, MODULE) });
    act(() => {
      result.current.setCreateOpen(false);
    });
    expect(result.current.createOpen).toBe(false);
  });
});
