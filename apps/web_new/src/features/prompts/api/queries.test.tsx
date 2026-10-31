// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { PromptSectionListResponse, ResolvedPromptSection } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, getAppClient, resetAppClient } from "../../../core/api/client.js";
import { LIST_STALE_TIME_MS, workspaceScope } from "../../../core/query/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { listPromptSections: vi.fn(), getCompiledPrompt: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { promptReceiptKeys, promptSectionKeys } from "./keys.js";
import {
  compiledPromptQueryOptions,
  promptSectionsQueryOptions,
  usePromptReceipts,
  usePromptSectionLists,
} from "./queries.js";

const SLUG = "acme";

function row(overrides: Partial<ResolvedPromptSection> = {}): ResolvedPromptSection {
  return {
    key: "autonomy",
    title: "Autonomy",
    customizable: "overridable",
    body: "effective body",
    default_body: "pristine default",
    source: "default",
    version: 1,
    needs_attention: false,
    editable_at_workspace: true,
    editable_at_personal: true,
    ...overrides,
  };
}

function listResponse(
  kind: string,
  scope: "user" | "workspace",
  sections: ResolvedPromptSection[]
): PromptSectionListResponse {
  return { kind, scope, sections };
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

describe("promptSectionKeys", () => {
  it("nests every list under the workspace prompts scope", () => {
    expect(promptSectionKeys.all(SLUG)).toEqual([...workspaceScope(SLUG), "prompts", "sections"]);
    expect(promptSectionKeys.list(SLUG, "review", "workspace")).toEqual([
      ...promptSectionKeys.all(SLUG),
      "review",
      "workspace",
    ]);
  });
});

describe("promptSectionsQueryOptions", () => {
  it("reads one kind at one scope with list freshness", async () => {
    contractMocks.listPromptSections.mockResolvedValue(listResponse("review", "user", [row()]));
    const options = promptSectionsQueryOptions(getAppClient(), SLUG, "review", "user");
    expect(options.queryKey).toEqual(promptSectionKeys.list(SLUG, "review", "user"));
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    await expect(options.queryFn?.({} as never)).resolves.toEqual({ kind: "review", sections: [row()] });
    expect(contractMocks.listPromptSections).toHaveBeenCalledWith(expect.anything(), SLUG, {
      kind: "review",
      scope: "user",
    });
  });
});

describe("usePromptSectionLists", () => {
  function mockKinds(
    member: (kind: string) => ResolvedPromptSection[],
    baseline: (kind: string) => ResolvedPromptSection[] = () => []
  ): void {
    contractMocks.listPromptSections.mockImplementation(
      (_client: unknown, _slug: string, options: { kind?: string; scope?: string }) => {
        const kind = options.kind ?? "coding-task";
        if (options.scope === "workspace") return Promise.resolve(listResponse(kind, "workspace", baseline(kind)));
        return Promise.resolve(listResponse(kind, "user", member(kind)));
      }
    );
  }

  it("merges the member lists and never fetches the baseline without it", async () => {
    mockKinds((kind) =>
      kind === "coding-task" ? [row({ key: "aaa", title: "Aaa" })] : [row({ key: "bbb", title: "Bbb" })]
    );
    const { result } = renderHook(() => usePromptSectionLists(SLUG, false), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.memberPending).toBe(false));
    expect(result.current.sections.map((section) => section.key)).toEqual(["aaa", "bbb"]);
    expect(result.current.memberFailed).toBe(false);
    expect(result.current.baselineFailed).toBe(false);
    expect(result.current.baselineLoaded).toBe(false);
    expect(result.current.baselinePending).toBe(false);
    for (const call of contractMocks.listPromptSections.mock.calls) {
      expect(call[2]).toMatchObject({ scope: "user" });
    }
  });

  it("merges the baseline for admins", async () => {
    mockKinds(
      () => [row({ key: "aaa", source: "workspace", body: "baseline body" })],
      () => [row({ key: "aaa", source: "workspace", body: "baseline body" })]
    );
    const { result } = renderHook(() => usePromptSectionLists(SLUG, true), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.baselineLoaded).toBe(true));
    expect(result.current.sections[0]?.workspace?.body).toBe("baseline body");
    expect(result.current.memberFailed).toBe(false);
    expect(result.current.baselineFailed).toBe(false);
  });

  it("reports a member-list failure", async () => {
    contractMocks.listPromptSections.mockImplementation(
      (_client: unknown, _slug: string, options: { kind?: string; scope?: string }) => {
        if (options.scope === "user" && options.kind === "review") return Promise.reject(new Error("boom"));
        return Promise.resolve(listResponse(options.kind ?? "coding-task", "user", [row()]));
      }
    );
    const { result } = renderHook(() => usePromptSectionLists(SLUG, false), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.memberFailed).toBe(true));
    expect(result.current.memberPending).toBe(false);
  });

  it("reports a baseline failure without failing the member lists", async () => {
    contractMocks.listPromptSections.mockImplementation(
      (_client: unknown, _slug: string, options: { kind?: string; scope?: string }) => {
        if (options.scope === "workspace") return Promise.reject(new Error("boom"));
        return Promise.resolve(listResponse(options.kind ?? "coding-task", "user", [row()]));
      }
    );
    const { result } = renderHook(() => usePromptSectionLists(SLUG, true), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.baselineFailed).toBe(true));
    expect(result.current.memberFailed).toBe(false);
    expect(result.current.baselineLoaded).toBe(false);
    expect(result.current.sections).toHaveLength(1);
  });

  it("reports pending while the lists resolve", () => {
    contractMocks.listPromptSections.mockReturnValue(new Promise<never>(() => {}));
    const { result } = renderHook(() => usePromptSectionLists(SLUG, true), { wrapper: wrapper(freshClient()) });
    expect(result.current.memberPending).toBe(true);
    expect(result.current.baselinePending).toBe(true);
    expect(result.current.memberFailed).toBe(false);
  });

  it("refetches the member lists on retry", async () => {
    mockKinds(() => [row()]);
    const { result } = renderHook(() => usePromptSectionLists(SLUG, false), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.memberPending).toBe(false));
    const calls = contractMocks.listPromptSections.mock.calls.length;
    result.current.refetchMember();
    await waitFor(() => expect(contractMocks.listPromptSections.mock.calls.length).toBeGreaterThan(calls));
  });
});

describe("promptReceiptKeys", () => {
  it("nests compiled reads under their own workspace subtree", () => {
    expect(promptReceiptKeys.all(SLUG)).toEqual([...workspaceScope(SLUG), "prompts", "receipts"]);
    expect(promptReceiptKeys.compiled(SLUG, "scheduler")).toEqual([...promptReceiptKeys.all(SLUG), "scheduler"]);
  });
});

describe("compiledPromptQueryOptions", () => {
  it("reads the viewer's own assembled template with list freshness", async () => {
    const compiled = { kind: "review", scope: "user" as const, template_body: "assembled" };
    contractMocks.getCompiledPrompt.mockResolvedValue(compiled);
    const options = compiledPromptQueryOptions(getAppClient(), SLUG, "review");
    expect(options.queryKey).toEqual(promptReceiptKeys.compiled(SLUG, "review"));
    expect(options.staleTime).toBe(LIST_STALE_TIME_MS);
    await expect(options.queryFn?.({} as never)).resolves.toEqual(compiled);
    expect(contractMocks.getCompiledPrompt).toHaveBeenCalledWith(expect.anything(), SLUG, "review", {
      scope: "user",
    });
  });
});

describe("usePromptReceipts", () => {
  function mockReceipts(options: { failCompiled?: boolean; failSections?: string; pending?: boolean } = {}): void {
    contractMocks.getCompiledPrompt.mockImplementation((_client: unknown, _slug: string, kind: string) =>
      options.pending
        ? new Promise<never>(() => {})
        : options.failCompiled
          ? Promise.reject(new Error("compiled failed"))
          : Promise.resolve({
              kind,
              scope: "user",
              template_body: `${kind} template`,
              ...(kind === "review" ? { automatic_template_body: "review automatic" } : {}),
            })
    );
    contractMocks.listPromptSections.mockImplementation(
      (_client: unknown, _slug: string, request: { kind?: string; scope?: string }) =>
        options.pending
          ? new Promise<never>(() => {})
          : options.failSections === request.kind
            ? Promise.reject(new Error("sections failed"))
            : Promise.resolve(listResponse(request.kind ?? "coding-task", "user", [row()]))
    );
  }

  it("pairs every kind's sections with its compiled template in page order", async () => {
    mockReceipts();
    const { result } = renderHook(() => usePromptReceipts(SLUG), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.pending).toBe(false));
    expect(result.current.failed).toBe(false);
    expect(result.current.entries.map((entry) => entry.kind)).toEqual(["coding-task", "review", "scheduler"]);
    expect(result.current.entries[1]?.compiled.template_body).toBe("review template");
    expect(result.current.entries[1]?.sections.sections).toHaveLength(1);
    expect(result.current.entries[2]?.compiled.automatic_template_body).toBeUndefined();
  });

  it("reports pending until every read settles", () => {
    mockReceipts({ pending: true });
    const { result } = renderHook(() => usePromptReceipts(SLUG), { wrapper: wrapper(freshClient()) });
    expect(result.current.pending).toBe(true);
    expect(result.current.failed).toBe(false);
    expect(result.current.entries).toEqual([]);
  });

  it("reports a compiled failure", async () => {
    mockReceipts({ failCompiled: true });
    const { result } = renderHook(() => usePromptReceipts(SLUG), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.failed).toBe(true));
    expect(result.current.pending).toBe(false);
  });

  it("reports a section-list failure on the receipt side", async () => {
    mockReceipts({ failSections: "scheduler" });
    const { result } = renderHook(() => usePromptReceipts(SLUG), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.failed).toBe(true));
  });

  it("refetches every receipt read", async () => {
    mockReceipts();
    const { result } = renderHook(() => usePromptReceipts(SLUG), { wrapper: wrapper(freshClient()) });
    await waitFor(() => expect(result.current.pending).toBe(false));
    const compiledCalls = contractMocks.getCompiledPrompt.mock.calls.length;
    const sectionCalls = contractMocks.listPromptSections.mock.calls.length;
    result.current.refetch();
    await waitFor(() => expect(contractMocks.getCompiledPrompt.mock.calls.length).toBeGreaterThan(compiledCalls));
    await waitFor(() => expect(contractMocks.listPromptSections.mock.calls.length).toBeGreaterThan(sectionCalls));
  });
});
