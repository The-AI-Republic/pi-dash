// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ addCycle: vi.fn(), setModules: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  addIssueToCycle: mocks.addCycle,
  setIssueModules: mocks.setModules,
}));

vi.mock("../../../core/api/client.js", () => ({ getAppClient: () => ({ stub: true }) }));

import { useApplyCreateMembership } from "./createApi.js";

describe("useApplyCreateMembership", () => {
  it("refreshes the workspace lists after the cycle and module writes settle", async () => {
    mocks.addCycle.mockResolvedValue({ message: "ok" });
    mocks.setModules.mockResolvedValue({ message: "ok" });
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const wrapper = ({ children }: { children: ReactNode }): ReactElement =>
      createElement(QueryClientProvider, { client: queryClient }, children);
    const { result } = renderHook(() => useApplyCreateMembership("acme"), { wrapper });
    result.current.mutate({ projectId: "project", issueId: "issue", cycleId: "cycle", moduleIds: ["module"] });
    await waitFor(() => expect(invalidate).toHaveBeenCalled());
    expect(invalidate.mock.calls[0]?.[0]).toEqual({
      queryKey: expect.arrayContaining(["issues", "list"]),
    });
    expect(mocks.addCycle).toHaveBeenCalled();
    expect(mocks.setModules).toHaveBeenCalled();
  });
});
