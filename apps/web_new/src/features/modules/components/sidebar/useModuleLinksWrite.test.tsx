// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError, type ModuleDetail, type ModuleLink } from "@pidash/api-client";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ createModuleLink: vi.fn(), updateModuleLink: vi.fn(), deleteModuleLink: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<Record<string, unknown>>();
  return {
    ...original,
    createModuleLink: mocks.createModuleLink,
    updateModuleLink: mocks.updateModuleLink,
    deleteModuleLink: mocks.deleteModuleLink,
  };
});

vi.mock("../../../../shared/shell/toaster.js", () => ({ notifyError: vi.fn(), notifySuccess: vi.fn() }));

import { notifyError, notifySuccess } from "../../../../shared/shell/toaster.js";
import { moduleKeys } from "../../api/keys.js";
import { makeModuleLink, MODULES_LINK_ID } from "../../testing.js";
import { MODULE_SAVE_FAILED_BODY } from "./moduleSidebarModel.js";
import { useModuleLinksWrite } from "./useModuleLinksWrite.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";
const MODULE = "323e4567-e89b-12d3-a456-426614174002";
const SECOND_LINK = "623e4567-e89b-12d3-a456-426614174005";

function setup(cached: ModuleLink[] | null = [makeModuleLink()]) {
  mocks.createModuleLink.mockReset();
  mocks.updateModuleLink.mockReset();
  mocks.deleteModuleLink.mockReset();
  const client = {} as never;
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  if (cached !== null) {
    queryClient.setQueryData(moduleKeys.detail(SLUG, PROJECT, MODULE), { id: MODULE, link_module: cached });
  }
  const wrapper = ({ children }: { children: React.ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  const { result } = renderHook(
    () => useModuleLinksWrite({ client, workspaceSlug: SLUG, projectId: PROJECT, moduleId: MODULE }),
    { wrapper }
  );
  const links = (): ModuleLink[] | undefined =>
    (queryClient.getQueryData(moduleKeys.detail(SLUG, PROJECT, MODULE)) as ModuleDetail | undefined)?.link_module;
  const calls = {
    creates: mocks.createModuleLink.mock.calls as unknown[][],
    updates: mocks.updateModuleLink.mock.calls as unknown[][],
    deletes: mocks.deleteModuleLink.mock.calls as unknown[][],
  };
  return { result, queryClient, client, links, calls };
}

describe("useModuleLinksWrite", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("appends the created link and confirms with the pinned notice", async () => {
    const created = makeModuleLink({ id: SECOND_LINK, title: "Second", url: "https://plain.example.com" });
    const { result, client, links, calls } = setup();
    mocks.createModuleLink.mockResolvedValue(created);
    await act(async () => {
      result.current.createLink.mutate({ url: "https://plain.example.com", title: "Second" });
    });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module link created successfully."));
    expect(calls.creates).toEqual([
      [client, SLUG, PROJECT, MODULE, { url: "https://plain.example.com", title: "Second" }],
    ]);
    expect(links()?.map((link) => link.id)).toEqual([MODULES_LINK_ID, SECOND_LINK]);
  });

  it("confirms a create against an absent record without writing one", async () => {
    const { result, links } = setup(null);
    mocks.createModuleLink.mockResolvedValue(makeModuleLink({ id: SECOND_LINK }));
    await act(async () => {
      result.current.createLink.mutate({ url: "https://plain.example.com" });
    });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module link created successfully."));
    expect(links()).toBeUndefined();
  });

  it("reports a refused create and leaves the cache untouched", async () => {
    const { result, links } = setup();
    mocks.createModuleLink.mockRejectedValue(
      new ApiError({ code: "http_error", message: "Enter a valid URL.", status: 400 })
    );
    await act(async () => {
      result.current.createLink.mutate({ url: "https://refused.example" });
    });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't create the module link", "Enter a valid URL.")
    );
    expect(links()?.map((link) => link.id)).toEqual([MODULES_LINK_ID]);
  });

  it("replaces the edited row in place and confirms", async () => {
    const second = makeModuleLink({ id: SECOND_LINK, title: "Second" });
    const { result, client, links, calls } = setup([makeModuleLink(), second]);
    const updated = { ...second, title: "Second v2" };
    mocks.updateModuleLink.mockResolvedValue(updated);
    await act(async () => {
      result.current.updateLink.mutate({ linkId: SECOND_LINK, input: { url: second.url, title: "Second v2" } });
    });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module link updated successfully."));
    expect(calls.updates).toEqual([
      [client, SLUG, PROJECT, MODULE, SECOND_LINK, { url: second.url, title: "Second v2" }],
    ]);
    expect(links()).toEqual([makeModuleLink(), updated]);
  });

  it("reports a refused update and keeps the previous row", async () => {
    const { result, links } = setup();
    mocks.updateModuleLink.mockRejectedValue(new ApiError({ code: "http_error", message: "", status: 500 }));
    await act(async () => {
      result.current.updateLink.mutate({ linkId: MODULES_LINK_ID, input: { url: "https://x.example" } });
    });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't update the module link", MODULE_SAVE_FAILED_BODY)
    );
    expect(links()).toEqual([makeModuleLink()]);
  });

  it("filters the deleted row and confirms", async () => {
    const second = makeModuleLink({ id: SECOND_LINK, title: "Second" });
    const { result, client, links, calls } = setup([makeModuleLink(), second]);
    mocks.deleteModuleLink.mockResolvedValue(undefined);
    await act(async () => {
      result.current.deleteLink.mutate(SECOND_LINK);
    });
    await waitFor(() => expect(notifySuccess).toHaveBeenCalledWith("Module link deleted successfully."));
    expect(calls.deletes).toEqual([[client, SLUG, PROJECT, MODULE, SECOND_LINK]]);
    expect(links()).toEqual([makeModuleLink()]);
  });

  it("reports a refused delete and keeps the row", async () => {
    const { result, links } = setup();
    mocks.deleteModuleLink.mockRejectedValue(new Error("gone"));
    await act(async () => {
      result.current.deleteLink.mutate(MODULES_LINK_ID);
    });
    await waitFor(() =>
      expect(notifyError).toHaveBeenCalledWith("Couldn't delete the module link", MODULE_SAVE_FAILED_BODY)
    );
    expect(links()).toEqual([makeModuleLink()]);
  });
});
