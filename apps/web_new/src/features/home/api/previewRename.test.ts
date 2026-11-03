// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RecentVisitList } from "@pidash/api-client";

import { previewRenameMutationOptions } from "./previewRename.js";
import { recentsKeys, recentsQueryOptions } from "./recents.js";

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
});

function stubClient() {
  const patch = vi.fn(async () => undefined);
  return { client: { patch } as never, patch };
}

function feed(): RecentVisitList {
  return [
    {
      id: "visit-1",
      entity_name: "issue",
      entity_identifier: "issue-1",
      entity_data: { id: "issue-1", name: "Before" },
      visited_at: "2026-10-14T11:00:00Z",
    },
    {
      id: "visit-2",
      entity_name: "issue",
      entity_identifier: "issue-2",
      entity_data: { id: "issue-2", name: "Other" },
      visited_at: "2026-10-14T11:00:00Z",
    },
    {
      id: "visit-3",
      entity_name: "project",
      entity_identifier: "project-1",
      entity_data: { id: "project-1", name: "Website" },
      visited_at: "2026-10-14T11:00:00Z",
    },
    {
      id: "visit-4",
      entity_name: "issue",
      entity_identifier: "issue-9",
      entity_data: null,
      visited_at: "2026-10-14T11:00:00Z",
    },
  ];
}

function titles(list: RecentVisitList | undefined): Array<string | null> {
  return (list ?? []).map((visit) => {
    const payload = visit.entity_data;
    if (payload === null) return null;
    const name: unknown = payload["name"];
    return typeof name === "string" ? name : null;
  });
}

describe("previewRenameMutationOptions", () => {
  it("patches the work-item name through the contract", async () => {
    const { client, patch } = stubClient();
    const options = previewRenameMutationOptions(client, new QueryClient(), "acme", "project-1", "issue-1");
    await options.mutationFn?.("After", {} as never);
    expect(patch).toHaveBeenCalledWith("/api/workspaces/acme/projects/project-1/issues/issue-1/", {
      json: { name: "After" },
    });
  });

  it("paints the rename across every recents filter cache", () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(recentsKeys.list("acme", "all"), feed());
    queryClient.setQueryData(recentsKeys.list("acme", "issue"), feed());
    const options = previewRenameMutationOptions(stubClient().client, queryClient, "acme", "project-1", "issue-1");
    options.onMutate?.("After", {} as never);
    expect(titles(queryClient.getQueryData(recentsKeys.list("acme", "all")))).toEqual([
      "After",
      "Other",
      "Website",
      null,
    ]);
    expect(titles(queryClient.getQueryData(recentsKeys.list("acme", "issue")))).toEqual([
      "After",
      "Other",
      "Website",
      null,
    ]);
  });

  it("rolls back every cache and toasts the server message on failure", async () => {
    const queryClient = new QueryClient();
    queryClient.setQueryData(recentsKeys.list("acme", "all"), feed());
    const options = previewRenameMutationOptions(stubClient().client, queryClient, "acme", "project-1", "issue-1");
    const context = await options.onMutate?.("After", {} as never);
    expect(titles(queryClient.getQueryData(recentsKeys.list("acme", "all")))?.[0]).toBe("After");
    options.onError?.(new Error("denied"), "After", context, {} as never);
    expect(titles(queryClient.getQueryData(recentsKeys.list("acme", "all")))?.[0]).toBe("Before");
    expect(notifyErrorMock).toHaveBeenCalledWith("Work item rename failed", "denied");
    expect(notifySuccessMock).not.toHaveBeenCalled();
  });

  it("tolerates empty caches and dataless entries on mutate", async () => {
    const empty = new QueryClient();
    const emptyOptions = previewRenameMutationOptions(stubClient().client, empty, "acme", "project-1", "issue-1");
    const emptyContext = await emptyOptions.onMutate?.("After", {} as never);
    expect(emptyContext?.rollbacks).toHaveLength(0);

    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const failing = {
      getJson: async () => Promise.reject(new Error("denied")),
      parse: (_schema: unknown, data: unknown) => data,
    } as never;
    await queryClient.prefetchQuery(recentsQueryOptions(failing, "acme", "page"));
    expect(queryClient.getQueryData(recentsKeys.list("acme", "page"))).toBeUndefined();
    const options = previewRenameMutationOptions(stubClient().client, queryClient, "acme", "project-1", "issue-1");
    const context = await options.onMutate?.("After", {} as never);
    expect(context?.rollbacks).toHaveLength(1);
    expect(queryClient.getQueryData(recentsKeys.list("acme", "page"))).toBeUndefined();
    options.onError?.(new Error("denied"), "After", context, {} as never);
    expect(queryClient.getQueryData(recentsKeys.list("acme", "page"))).toBeUndefined();
  });

  it("toasts without a description for non-error failures", () => {
    const queryClient = new QueryClient();
    const options = previewRenameMutationOptions(stubClient().client, queryClient, "acme", "project-1", "issue-1");
    options.onError?.("boom", "After", undefined, {} as never);
    expect(notifyErrorMock).toHaveBeenCalledWith("Work item rename failed", undefined);
  });

  it("confirms the rename on success", () => {
    const options = previewRenameMutationOptions(
      stubClient().client,
      new QueryClient(),
      "acme",
      "project-1",
      "issue-1"
    );
    options.onSuccess?.(undefined as never, "After", { rollbacks: [] }, {} as never);
    expect(notifySuccessMock).toHaveBeenCalledWith("Work item renamed");
  });

  it("refreshes the recents reads once settled", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const options = previewRenameMutationOptions(stubClient().client, queryClient, "acme", "project-1", "issue-1");
    options.onSettled?.(undefined, null, "After", undefined, undefined as never);
    expect(invalidate).toHaveBeenCalledTimes(1);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: recentsKeys.all("acme") });
  });
});
