// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { upsertPromptSection: vi.fn(), revertPromptSection: vi.fn(), previewPrompt: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

vi.mock("../../../shared/shell/index.js", () => ({
  notifySuccess: vi.fn(),
}));

import { notifySuccess } from "../../../shared/shell/index.js";
import { promptReceiptKeys, promptSectionKeys } from "./keys.js";
import {
  PERSONAL_SAVED_TITLE,
  REVERT_FAILED_MESSAGE,
  SECTION_REVERTED_TITLE,
  saveFailureDetail,
  saveFailureMessage,
  usePreviewPrompt,
  useRevertPromptSection,
  useUpsertPromptSection,
  WORKSPACE_SAVED_TITLE,
} from "./mutations.js";

const SLUG = "acme";

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

function freshClient(): QueryClient {
  return new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } });
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
});

describe("saveFailureDetail", () => {
  it("prefers the first field message on validation refusals", () => {
    const error = new ApiError({
      status: 400,
      code: "http",
      message: "Request failed",
      fields: { body: ["Too long."] },
    });
    expect(saveFailureDetail(error)).toBe("Too long.");
  });

  it("skips empty field lists for the message", () => {
    const error = new ApiError({ status: 400, code: "http", message: "Bad body.", fields: { body: [] } });
    expect(saveFailureDetail(error)).toBe("Bad body.");
  });

  it("falls back when the API error carries no message", () => {
    expect(saveFailureDetail(new ApiError({ status: 500, code: "http", message: "" }))).toBe(
      "The server gave no reason."
    );
  });

  it("reads plain errors and unknowns", () => {
    expect(saveFailureDetail(new Error("Network down."))).toBe("Network down.");
    expect(saveFailureDetail(new Error(""))).toBe("The server gave no reason.");
    expect(saveFailureDetail("parity upsert failure")).toBe("The server gave no reason.");
    expect(saveFailureDetail(null)).toBe("The server gave no reason.");
  });
});

describe("saveFailureMessage", () => {
  it("frames the server detail", () => {
    expect(saveFailureMessage(new Error("parity upsert failure"))).toBe(
      "Could not save the section. parity upsert failure"
    );
  });

  it("keeps the revert message generic", () => {
    expect(REVERT_FAILED_MESSAGE).toBe("Could not revert the section.");
  });
});

describe("useUpsertPromptSection", () => {
  const stored = {
    id: "123e4567-e89b-12d3-a456-426614174000",
    workspace: "223e4567-e89b-12d3-a456-426614174000",
    user: null,
    section_key: "autonomy",
    body: "new body",
    is_active: true,
    version: 2,
    needs_attention: false,
    is_workspace_level: true,
    updated_by: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
  };

  it("saves the workspace default, toasts and refreshes the lists", async () => {
    contractMocks.upsertPromptSection.mockResolvedValue(stored);
    const queryClient = freshClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useUpsertPromptSection(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      result.current.mutate({ key: "autonomy", scope: "workspace", body: "new body" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.upsertPromptSection).toHaveBeenCalledWith(expect.anything(), SLUG, "autonomy", {
      scope: "workspace",
      body: "new body",
    });
    expect(notifySuccess).toHaveBeenCalledWith(WORKSPACE_SAVED_TITLE, expect.any(String));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: promptSectionKeys.all(SLUG) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: promptReceiptKeys.all(SLUG) });
  });

  it("toasts the personal title at user scope", async () => {
    contractMocks.upsertPromptSection.mockResolvedValue({ ...stored, is_workspace_level: false });
    const { result } = renderHook(() => useUpsertPromptSection(SLUG), { wrapper: wrapper(freshClient()) });
    await act(async () => {
      result.current.mutate({ key: "autonomy", scope: "user", body: "mine" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith(PERSONAL_SAVED_TITLE, expect.any(String));
  });
});

describe("useRevertPromptSection", () => {
  it("reverts the workspace override, toasts and refreshes the lists", async () => {
    contractMocks.revertPromptSection.mockResolvedValue(204);
    const queryClient = freshClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => useRevertPromptSection(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      result.current.mutate({ key: "autonomy", scope: "workspace" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.revertPromptSection).toHaveBeenCalledWith(expect.anything(), SLUG, "autonomy", "workspace");
    expect(notifySuccess).toHaveBeenCalledWith(SECTION_REVERTED_TITLE, expect.stringContaining("workspace"));
    expect(invalidate).toHaveBeenCalledWith({ queryKey: promptSectionKeys.all(SLUG) });
    expect(invalidate).toHaveBeenCalledWith({ queryKey: promptReceiptKeys.all(SLUG) });
  });

  it("toasts the personal body at user scope", async () => {
    contractMocks.revertPromptSection.mockResolvedValue(204);
    const { result } = renderHook(() => useRevertPromptSection(SLUG), { wrapper: wrapper(freshClient()) });
    await act(async () => {
      result.current.mutate({ key: "autonomy", scope: "user" });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(notifySuccess).toHaveBeenCalledWith(SECTION_REVERTED_TITLE, expect.stringContaining("Your override"));
  });
});

describe("usePreviewPrompt", () => {
  it("posts the kind and target through the contract without invalidating reads", async () => {
    contractMocks.previewPrompt.mockResolvedValue({ kind: "coding-task", prompt: "rendered" });
    const queryClient = freshClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const { result } = renderHook(() => usePreviewPrompt(SLUG), { wrapper: wrapper(queryClient) });
    await act(async () => {
      result.current.mutate({ kind: "coding-task", input: { issue_id: "issue-1" } });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual({ kind: "coding-task", prompt: "rendered" });
    expect(contractMocks.previewPrompt).toHaveBeenCalledWith(expect.anything(), SLUG, "coding-task", {
      issue_id: "issue-1",
    });
    expect(invalidate).not.toHaveBeenCalled();
  });

  it("carries the draft fields alongside the install target", async () => {
    contractMocks.previewPrompt.mockResolvedValue({ kind: "scheduler", prompt: "draft" });
    const { result } = renderHook(() => usePreviewPrompt(SLUG), { wrapper: wrapper(freshClient()) });
    await act(async () => {
      result.current.mutate({
        kind: "scheduler",
        input: { binding_id: "binding-1", scope: "user", section_key: "brief", body: "draft body" },
      });
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(contractMocks.previewPrompt).toHaveBeenCalledWith(expect.anything(), SLUG, "scheduler", {
      binding_id: "binding-1",
      scope: "user",
      section_key: "brief",
      body: "draft body",
    });
  });

  it("surfaces the failure to the caller for the inline message", async () => {
    contractMocks.previewPrompt.mockRejectedValue(new Error("parity preview failure"));
    const { result } = renderHook(() => usePreviewPrompt(SLUG), { wrapper: wrapper(freshClient()) });
    await act(async () => {
      result.current.mutate({ kind: "review", input: { issue_id: "issue-1" } });
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.error?.message).toBe("parity preview failure");
  });
});
