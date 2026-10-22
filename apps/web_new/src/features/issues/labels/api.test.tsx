// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError, type Label, type LabelList } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: {
    listLabels: vi.fn(),
    listWorkspaceLabels: vi.fn(),
    createLabel: vi.fn(),
    updateLabel: vi.fn(),
    deleteLabel: vi.fn(),
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/shell/index.js", () => ({
  notifyError: vi.fn(),
}));

import { notifyError } from "../../../shared/shell/index.js";
import {
  appendCachedLabel,
  applyLabelPatch,
  isLabelDuplicateError,
  LABEL_CREATE_FAILED_TITLE,
  LABEL_DELETE_FAILED_TITLE,
  LABEL_DUPLICATE_MESSAGE,
  LABEL_FAILURE_FALLBACK,
  LABEL_RENAME_DUPLICATE_MESSAGE,
  LABEL_UPDATE_FAILED_TITLE,
  labelFailureDetail,
  labelKeys,
  removeCachedLabel,
  useCreateLabel,
  useDeleteLabel,
  useLabels,
  useMoveLabel,
  useUpdateLabel,
  useWorkspaceLabels,
  workspaceLabelsQueryOptions,
} from "./api.js";

const SLUG = "acme";
const PROJECT = "223e4567-e89b-12d3-a456-426614174001";

function row(id: string, name: string, sort: number, parent: string | null = null): Label {
  return {
    id,
    project_id: PROJECT,
    workspace_id: "323e4567-e89b-12d3-a456-426614174002",
    name,
    color: "#FF6900",
    parent,
    sort_order: sort,
  };
}

function duplicateError(): ApiError {
  return new ApiError({
    status: 400,
    code: "http",
    message: "name: LABEL_NAME_ALREADY_EXISTS",
    fields: { name: ["LABEL_NAME_ALREADY_EXISTS"] },
  });
}

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function freshClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false } } });
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("labelKeys", () => {
  it("nests the project list under the workspace labels scope", () => {
    expect(labelKeys.list(SLUG, PROJECT)).toEqual([...labelKeys.all(SLUG), "list", PROJECT]);
  });

  it("keys the workspace list without a project segment", () => {
    expect(labelKeys.workspace(SLUG)).toEqual([...labelKeys.all(SLUG), "workspace-list"]);
  });
});

describe("isLabelDuplicateError", () => {
  it("matches a 400 carrying name-field errors", () => {
    expect(isLabelDuplicateError(duplicateError())).toBe(true);
  });

  it("rejects anything else", () => {
    expect(isLabelDuplicateError(new Error("nope"))).toBe(false);
    expect(isLabelDuplicateError("nope")).toBe(false);
    expect(isLabelDuplicateError(new ApiError({ status: 500, code: "http", message: "x" }))).toBe(false);
    expect(isLabelDuplicateError(new ApiError({ status: 400, code: "http", message: "x" }))).toBe(false);
    expect(isLabelDuplicateError(new ApiError({ status: 400, code: "http", message: "x", fields: { name: [] } }))).toBe(
      false
    );
  });
});

describe("labelFailureDetail", () => {
  it("prefers the server message, falling back when empty", () => {
    expect(labelFailureDetail(new ApiError({ status: 500, code: "http", message: "Down" }))).toBe("Down");
    expect(labelFailureDetail(new ApiError({ status: 500, code: "http", message: "" }))).toBe(LABEL_FAILURE_FALLBACK);
    expect(labelFailureDetail(new Error("nope"))).toBe(LABEL_FAILURE_FALLBACK);
  });
});

describe("label cache helpers", () => {
  const labels = [row("a", "A", 10000), row("b", "B", 20000)];

  it("patches one row, passing undefined through", () => {
    expect(applyLabelPatch(undefined, "a", { name: "X" })).toBe(undefined);
    expect(applyLabelPatch(labels, "b", { name: "X", color: "#00D084" })).toEqual([
      labels[0],
      { ...labels[1], name: "X", color: "#00D084" },
    ]);
  });

  it("removes one row, passing undefined through", () => {
    expect(removeCachedLabel(undefined, "a")).toBe(undefined);
    expect(removeCachedLabel(labels, "a")).toEqual([labels[1]]);
  });

  it("appends the created row, starting a fresh list when empty", () => {
    const created = row("c", "C", 30000);
    expect(appendCachedLabel(undefined, created)).toEqual([created]);
    expect(appendCachedLabel(labels, created)).toEqual([...labels, created]);
  });
});

describe("useLabels", () => {
  it("reads the project list through the contract", async () => {
    const labels = [row("a", "A", 10000)];
    contractMocks.listLabels.mockResolvedValue(labels);
    const queryClient = freshClient();
    const { result } = renderHook(() => useLabels(SLUG, PROJECT), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(labels);
    expect(contractMocks.listLabels).toHaveBeenCalledTimes(1);
  });

  it("stays idle until enabled", () => {
    const queryClient = freshClient();
    const { result } = renderHook(() => useLabels(SLUG, PROJECT, false), {
      wrapper: wrapper(queryClient),
    });
    expect(result.current.isPending).toBe(true);
    expect(result.current.fetchStatus).toBe("idle");
    expect(contractMocks.listLabels).not.toHaveBeenCalled();
  });

  it("never fires the project read for an empty project id", () => {
    const queryClient = freshClient();
    const { result } = renderHook(() => useLabels(SLUG, ""), { wrapper: wrapper(queryClient) });
    expect(result.current.fetchStatus).toBe("idle");
    expect(contractMocks.listLabels).not.toHaveBeenCalled();
  });
});

describe("useWorkspaceLabels", () => {
  it("reads the workspace list through the contract", async () => {
    const labels = [row("a", "A", 10000)];
    contractMocks.listWorkspaceLabels.mockResolvedValue(labels);
    const queryClient = freshClient();
    const { result } = renderHook(() => useWorkspaceLabels(SLUG), { wrapper: wrapper(queryClient) });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual(labels);
    expect(contractMocks.listWorkspaceLabels).toHaveBeenCalledTimes(1);
  });

  it("stays idle until enabled", () => {
    const queryClient = freshClient();
    const { result } = renderHook(() => useWorkspaceLabels(SLUG, false), {
      wrapper: wrapper(queryClient),
    });
    expect(result.current.isPending).toBe(true);
    expect(result.current.fetchStatus).toBe("idle");
    expect(contractMocks.listWorkspaceLabels).not.toHaveBeenCalled();
  });

  it("keys the read under the workspace list", () => {
    const options = workspaceLabelsQueryOptions({} as never, SLUG);
    expect(options.queryKey).toEqual(labelKeys.workspace(SLUG));
  });
});

describe("useCreateLabel", () => {
  it("appends the server row to the shared list", async () => {
    const created = row("c", "C", 30000);
    contractMocks.createLabel.mockResolvedValue(created);
    const queryClient = freshClient();
    queryClient.setQueryData<LabelList>(labelKeys.list(SLUG, PROJECT), [row("a", "A", 10000)]);
    const { result } = renderHook(() => useCreateLabel(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    let saved: Label | undefined;
    await act(async () => {
      saved = await result.current.mutateAsync({ name: "C", color: "#00D084" });
    });
    expect(saved).toEqual(created);
    expect(queryClient.getQueryData<LabelList>(labelKeys.list(SLUG, PROJECT))).toEqual([row("a", "A", 10000), created]);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("starts the list when the cache is empty", async () => {
    const created = row("c", "C", 30000);
    contractMocks.createLabel.mockResolvedValue(created);
    const queryClient = freshClient();
    const { result } = renderHook(() => useCreateLabel(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ name: "C", color: "#00D084" });
    });
    expect(queryClient.getQueryData<LabelList>(labelKeys.list(SLUG, PROJECT))).toEqual([created]);
  });

  it("notices duplicates distinctly from other failures", async () => {
    contractMocks.createLabel.mockRejectedValueOnce(duplicateError());
    contractMocks.createLabel.mockRejectedValueOnce(new ApiError({ status: 500, code: "http", message: "Down" }));
    const queryClient = freshClient();
    const { result } = renderHook(() => useCreateLabel(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    await act(async () => {
      await expect(result.current.mutateAsync({ name: "A", color: "#FF6900" })).rejects.toThrow();
    });
    expect(notifyError).toHaveBeenCalledWith(LABEL_CREATE_FAILED_TITLE, LABEL_DUPLICATE_MESSAGE);
    expect(LABEL_DUPLICATE_MESSAGE).toBe("Label already exists");
    await act(async () => {
      await expect(result.current.mutateAsync({ name: "B", color: "#FF6900" })).rejects.toThrow();
    });
    expect(notifyError).toHaveBeenCalledWith(LABEL_CREATE_FAILED_TITLE, "Down");
    expect(queryClient.getQueryData(labelKeys.list(SLUG, PROJECT))).toBe(undefined);
  });
});

describe("useUpdateLabel", () => {
  it("paints the patch optimistically, then lands the server row", async () => {
    const labels = [row("a", "A", 10000)];
    let release!: (value: Label) => void;
    contractMocks.updateLabel.mockReturnValue(new Promise<Label>((resolve) => (release = resolve)));
    const queryClient = freshClient();
    queryClient.setQueryData<LabelList>(labelKeys.list(SLUG, PROJECT), labels);
    const { result } = renderHook(() => useUpdateLabel(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    let pending: Promise<Label>;
    act(() => {
      pending = result.current.mutateAsync({ labelId: "a", patch: { name: "Edited" } });
    });
    await waitFor(() =>
      expect(queryClient.getQueryData<LabelList>(labelKeys.list(SLUG, PROJECT))).toEqual([
        { ...labels[0], name: "Edited" },
      ])
    );
    const server = { ...labels[0]!, name: "Edited", color: "#00D084" };
    act(() => {
      release(server);
    });
    await act(async () => {
      await pending;
    });
    expect(queryClient.getQueryData<LabelList>(labelKeys.list(SLUG, PROJECT))).toEqual([server]);
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("rolls back and notices on failure, duplicates distinctly", async () => {
    const labels = [row("a", "A", 10000)];
    contractMocks.updateLabel.mockRejectedValueOnce(new ApiError({ status: 404, code: "http", message: "Gone" }));
    contractMocks.updateLabel.mockRejectedValueOnce(duplicateError());
    const queryClient = freshClient();
    queryClient.setQueryData<LabelList>(labelKeys.list(SLUG, PROJECT), labels);
    const { result } = renderHook(() => useUpdateLabel(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    await act(async () => {
      await expect(result.current.mutateAsync({ labelId: "a", patch: { name: "Ghost" } })).rejects.toThrow();
    });
    expect(queryClient.getQueryData<LabelList>(labelKeys.list(SLUG, PROJECT))).toEqual(labels);
    expect(notifyError).toHaveBeenCalledWith(LABEL_UPDATE_FAILED_TITLE, "Gone");
    expect(LABEL_UPDATE_FAILED_TITLE).toBe("Error while updating the label");
    await act(async () => {
      await expect(result.current.mutateAsync({ labelId: "a", patch: { name: "Clash" } })).rejects.toThrow();
    });
    expect(notifyError).toHaveBeenCalledWith(LABEL_UPDATE_FAILED_TITLE, LABEL_RENAME_DUPLICATE_MESSAGE);
    expect(LABEL_RENAME_DUPLICATE_MESSAGE).toContain("same name already exists");
  });
});

describe("useDeleteLabel", () => {
  it("removes the row, restoring it when the delete fails", async () => {
    const labels = [row("a", "A", 10000), row("b", "B", 20000)];
    contractMocks.deleteLabel.mockResolvedValueOnce(undefined);
    contractMocks.deleteLabel.mockRejectedValueOnce(new ApiError({ status: 500, code: "http", message: "Down" }));
    const queryClient = freshClient();
    queryClient.setQueryData<LabelList>(labelKeys.list(SLUG, PROJECT), labels);
    const { result } = renderHook(() => useDeleteLabel(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync("a");
    });
    expect(queryClient.getQueryData<LabelList>(labelKeys.list(SLUG, PROJECT))).toEqual([labels[1]]);
    expect(notifyError).not.toHaveBeenCalled();
    await act(async () => {
      await expect(result.current.mutateAsync("b")).rejects.toThrow();
    });
    expect(queryClient.getQueryData<LabelList>(labelKeys.list(SLUG, PROJECT))).toEqual([labels[1]]);
    expect(notifyError).toHaveBeenCalledWith(LABEL_DELETE_FAILED_TITLE);
    expect(LABEL_DELETE_FAILED_TITLE).toBe("Label could not be deleted. Please try again.");
  });
});

describe("useMoveLabel", () => {
  it("sends the resolved parent with and without an order", async () => {
    const labels = [row("a", "A", 10000)];
    contractMocks.updateLabel.mockImplementation(
      async (_client: unknown, _slug: string, _project: string, id: string, patch: Record<string, unknown>) =>
        ({ ...labels[0], id, ...patch }) as Label
    );
    const queryClient = freshClient();
    queryClient.setQueryData<LabelList>(labelKeys.list(SLUG, PROJECT), labels);
    const { result } = renderHook(() => useMoveLabel(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    await act(async () => {
      await result.current.mutateAsync({ labelId: "a", parent: null, sortOrder: 5000 });
    });
    expect(contractMocks.updateLabel).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "a", {
      parent: null,
      sort_order: 5000,
    });
    await act(async () => {
      await result.current.mutateAsync({ labelId: "a", parent: "p" });
    });
    expect(contractMocks.updateLabel).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, "a", {
      parent: "p",
    });
    expect(notifyError).not.toHaveBeenCalled();
  });

  it("rolls back silently when the save fails", async () => {
    const labels = [row("a", "A", 10000)];
    contractMocks.updateLabel.mockRejectedValueOnce(new ApiError({ status: 500, code: "http", message: "Down" }));
    const queryClient = freshClient();
    queryClient.setQueryData<LabelList>(labelKeys.list(SLUG, PROJECT), labels);
    const { result } = renderHook(() => useMoveLabel(SLUG, PROJECT), {
      wrapper: wrapper(queryClient),
    });
    await act(async () => {
      await expect(result.current.mutateAsync({ labelId: "a", parent: null, sortOrder: 5000 })).rejects.toThrow();
    });
    expect(queryClient.getQueryData<LabelList>(labelKeys.list(SLUG, PROJECT))).toEqual(labels);
    expect(notifyError).not.toHaveBeenCalled();
  });
});
