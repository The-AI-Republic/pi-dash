// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { QuickLinkList } from "@pidash/api-client";

import {
  createQuickLinkMutationOptions,
  deleteQuickLinkMutationOptions,
  quicklinksKeys,
  quicklinksQueryOptions,
  updateQuickLinkMutationOptions,
} from "./quicklinks.js";

const { notifyErrorMock, notifySuccessMock } = vi.hoisted(() => ({
  notifyErrorMock: vi.fn(),
  notifySuccessMock: vi.fn(),
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...actual, notifyError: notifyErrorMock, notifySuccess: notifySuccessMock };
});

beforeEach(() => {
  vi.spyOn(console, "error").mockImplementation(() => {});
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.resetAllMocks();
});

function stubClient() {
  const getJson = vi.fn(async () => []);
  const sendJson = vi.fn(async () => ({ data: {}, status: 200, url: "https://api.test/" }));
  const remove = vi.fn(async () => ({ status: 204 }));
  const client = {
    getJson,
    sendJson,
    delete: remove,
    parse: (_schema: unknown, data: unknown) => data,
  } as never;
  return { client, getJson, sendJson, remove };
}

function link(id: string, title: string | null, url: string): QuickLinkList[number] {
  return {
    id,
    workspace: "workspace-1",
    owner: "owner-1",
    title,
    url,
    metadata: {},
    created_by: null,
    updated_by: null,
    created_at: "2026-10-01T00:00:00Z",
    updated_at: "2026-10-02T00:00:00Z",
  };
}

function titles(list: QuickLinkList | undefined): Array<string | null> {
  return (list ?? []).map((entry) => entry.title);
}

describe("quicklinksKeys", () => {
  it("scopes the links under the workspace", () => {
    expect(quicklinksKeys.all("acme")).toEqual(["ws", "acme", "home", "quick-links"]);
    expect(quicklinksKeys.list("acme")).toEqual(["ws", "acme", "home", "quick-links", "list"]);
  });
});

describe("quicklinksQueryOptions", () => {
  it("reads with list freshness and no silent refetch", () => {
    const options = quicklinksQueryOptions(stubClient().client, "acme");
    expect(options.queryKey).toEqual(["ws", "acme", "home", "quick-links", "list"]);
    expect(options.staleTime).toBe(30_000);
    expect(options.refetchOnWindowFocus).toBe(false);
    expect(options.refetchOnReconnect).toBe(false);
  });

  it("lists the links through the contract", async () => {
    const { client, getJson } = stubClient();
    const options = quicklinksQueryOptions(client, "acme");
    await expect(options.queryFn?.({} as never)).resolves.toEqual([]);
    expect(getJson).toHaveBeenCalledWith("/api/workspaces/acme/quick-links/");
  });
});

describe("createQuickLinkMutationOptions", () => {
  it("posts the draft through the contract", async () => {
    const { client, sendJson } = stubClient();
    const options = createQuickLinkMutationOptions(client, new QueryClient(), "acme");
    await options.mutationFn?.({ url: "https://example.com", title: "Docs" }, {} as never);
    expect(sendJson).toHaveBeenCalledWith("POST", "/api/workspaces/acme/quick-links/", {
      json: { url: "https://example.com", title: "Docs" },
    });
  });

  it("omits a blank display name on create", async () => {
    const { client, sendJson } = stubClient();
    const options = createQuickLinkMutationOptions(client, new QueryClient(), "acme");
    await options.mutationFn?.({ url: "https://example.com", title: "" }, {} as never);
    expect(sendJson).toHaveBeenCalledWith("POST", "/api/workspaces/acme/quick-links/", {
      json: { url: "https://example.com" },
    });
  });

  it("appends an optimistic row", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(quicklinksKeys.list("acme"), [link("link-1", "Docs", "https://example.com")]);
    const options = createQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    await options.onMutate?.({ url: "https://example.org", title: "Guide" }, {} as never);
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual(["Docs", "Guide"]);
  });

  it("starts the list from an empty cache", async () => {
    const queryClient = new QueryClient();
    const options = createQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    await options.onMutate?.({ url: "https://example.org", title: "Guide" }, {} as never);
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual(["Guide"]);
  });

  it("rolls back and toasts the server message on failure", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(quicklinksKeys.list("acme"), [link("link-1", "Docs", "https://example.com")]);
    const options = createQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    const context = await options.onMutate?.({ url: "https://example.org", title: "Guide" }, {} as never);
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual(["Docs", "Guide"]);
    options.onError?.(new Error("denied"), { url: "https://example.org", title: "Guide" }, context, {} as never);
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual(["Docs"]);
    expect(notifyErrorMock).toHaveBeenCalledWith("Link save failed", "denied");
    expect(notifySuccessMock).not.toHaveBeenCalled();
  });

  it("toasts without a description for non-error failures", () => {
    const options = createQuickLinkMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onError?.("boom", { url: "https://example.org", title: "Guide" }, undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith("Link save failed", undefined);
  });

  it("confirms the save on success", () => {
    const options = createQuickLinkMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onSuccess?.({} as never, { url: "https://example.org", title: "Guide" }, { rollbacks: [] }, {} as never);
    expect(notifySuccessMock).toHaveBeenCalledWith("Link saved");
  });

  it("refreshes the list once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = createQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    options.onSettled?.(undefined, null, { url: "https://example.org", title: "Guide" }, undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: quicklinksKeys.all("acme") });
  });
});

describe("updateQuickLinkMutationOptions", () => {
  it("patches the row through the contract", async () => {
    const { client, sendJson } = stubClient();
    const options = updateQuickLinkMutationOptions(client, new QueryClient(), "acme");
    await options.mutationFn?.({ id: "link-1", url: "https://example.org", title: "Guide" }, {} as never);
    expect(sendJson).toHaveBeenCalledWith("PATCH", "/api/workspaces/acme/quick-links/link-1/", {
      json: { url: "https://example.org", title: "Guide" },
    });
  });

  it("paints the edit and leaves other rows alone", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(quicklinksKeys.list("acme"), [
      link("link-1", "Docs", "https://example.com"),
      link("link-2", "Other", "https://other.example"),
    ]);
    const options = updateQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    await options.onMutate?.({ id: "link-1", url: "https://example.org", title: "Guide" }, {} as never);
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual(["Guide", "Other"]);
  });

  it("tolerates an empty cache on mutate", async () => {
    const queryClient = new QueryClient();
    const options = updateQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    const context = await options.onMutate?.({ id: "link-1", url: "https://example.org", title: "Guide" }, {} as never);
    expect(queryClient.getQueryData(quicklinksKeys.list("acme"))).toBeUndefined();
    options.onError?.(new Error("denied"), { id: "link-1", url: "", title: "" }, context, {} as never);
    expect(queryClient.getQueryData(quicklinksKeys.list("acme"))).toBeUndefined();
  });

  it("rolls back and toasts the server message on failure", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(quicklinksKeys.list("acme"), [link("link-1", "Docs", "https://example.com")]);
    const options = updateQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    const context = await options.onMutate?.({ id: "link-1", url: "https://example.org", title: "Guide" }, {} as never);
    options.onError?.(
      new Error("denied"),
      { id: "link-1", url: "https://example.org", title: "Guide" },
      context,
      {} as never
    );
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual(["Docs"]);
    expect(notifyErrorMock).toHaveBeenCalledWith("Link update failed", "denied");
    expect(notifySuccessMock).not.toHaveBeenCalled();
  });

  it("toasts without a description for non-error failures", () => {
    const options = updateQuickLinkMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onError?.("boom", { id: "link-1", url: "", title: "" }, undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith("Link update failed", undefined);
  });

  it("confirms the edit on success", () => {
    const options = updateQuickLinkMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onSuccess?.({} as never, { id: "link-1", url: "", title: "" }, { rollbacks: [] }, {} as never);
    expect(notifySuccessMock).toHaveBeenCalledWith("Link updated");
  });

  it("refreshes the list once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = updateQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    options.onSettled?.(undefined, null, { id: "link-1", url: "", title: "" }, undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: quicklinksKeys.all("acme") });
  });
});

describe("deleteQuickLinkMutationOptions", () => {
  it("deletes the row through the contract", async () => {
    const { client, remove } = stubClient();
    const options = deleteQuickLinkMutationOptions(client, new QueryClient(), "acme");
    await options.mutationFn?.("link-1", {} as never);
    expect(remove).toHaveBeenCalledWith("/api/workspaces/acme/quick-links/link-1/");
  });

  it("drops the row and leaves other rows alone", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(quicklinksKeys.list("acme"), [
      link("link-1", "Docs", "https://example.com"),
      link("link-2", "Other", "https://other.example"),
    ]);
    const options = deleteQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    await options.onMutate?.("link-1", {} as never);
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual(["Other"]);
  });

  it("tolerates an empty cache on mutate", async () => {
    const queryClient = new QueryClient();
    const options = deleteQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    await options.onMutate?.("link-1", {} as never);
    expect(queryClient.getQueryData(quicklinksKeys.list("acme"))).toBeUndefined();
  });

  it("rolls back and toasts the server message on failure", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(quicklinksKeys.list("acme"), [link("link-1", "Docs", "https://example.com")]);
    const options = deleteQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    const context = await options.onMutate?.("link-1", {} as never);
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual([]);
    options.onError?.(new Error("denied"), "link-1", context, {} as never);
    expect(titles(queryClient.getQueryData(quicklinksKeys.list("acme")))).toEqual(["Docs"]);
    expect(notifyErrorMock).toHaveBeenCalledWith("Link delete failed", "denied");
    expect(notifySuccessMock).not.toHaveBeenCalled();
  });

  it("toasts without a description for non-error failures", () => {
    const options = deleteQuickLinkMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onError?.("boom", "link-1", undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith("Link delete failed", undefined);
  });

  it("confirms the removal on success", () => {
    const options = deleteQuickLinkMutationOptions(stubClient().client, new QueryClient(), "acme");
    options.onSuccess?.(204 as never, "link-1", { rollbacks: [] }, {} as never);
    expect(notifySuccessMock).toHaveBeenCalledWith("Link deleted");
  });

  it("refreshes the list once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = deleteQuickLinkMutationOptions(stubClient().client, queryClient, "acme");
    options.onSettled?.(undefined, null, "link-1", undefined, {} as never);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: quicklinksKeys.all("acme") });
  });
});
