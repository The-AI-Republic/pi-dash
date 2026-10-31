// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Occurrence reads (parity/specs/agents.md §5, AGT-015/017): the window is
// keyed by workspace, project and both bounds, a window wider than the
// server cap is pulled back to 90 days, and the hook surfaces the read's
// truncation flag for the calendar header.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../core/api/client.js";

const { contracts } = vi.hoisted(() => ({ contracts: { listSchedulerOccurrences: vi.fn() } }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});

import {
  MAX_WINDOW_DAYS,
  TRUNCATION_NOTICE,
  clampWindow,
  occurrenceKeys,
  occurrencesQueryOptions,
  useSchedulerOccurrences,
} from "./occurrences.js";

const SLUG = "acme";
const PROJECT = "p1";
const FROM = "2026-10-18T00:00:00.000Z";
const TO = "2026-10-24T23:59:59.999Z";

function wrapper(queryClient: QueryClient): React.ComponentType<{ children?: unknown }> {
  return function Wrapper({ children }: { children?: unknown }): React.ReactElement {
    return <QueryClientProvider client={queryClient}>{children as never}</QueryClientProvider>;
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});
afterEach(() => resetAppClient());

describe("occurrenceKeys", () => {
  it("scopes a window under the workspace and project", () => {
    expect(occurrenceKeys.all(SLUG, PROJECT)).toEqual(["ws", SLUG, "scheduler-occurrences", PROJECT]);
    expect(occurrenceKeys.window(SLUG, PROJECT, { from: FROM, to: TO })).toEqual([
      "ws",
      SLUG,
      "scheduler-occurrences",
      PROJECT,
      FROM,
      TO,
    ]);
  });

  it("names the truncation hint", () => {
    expect(TRUNCATION_NOTICE).toBe("Too many occurrences in this window");
  });
});

describe("clampWindow", () => {
  it("leaves a window inside the cap untouched, identity included", () => {
    const window = { from: FROM, to: TO };
    expect(clampWindow(window)).toBe(window);
  });

  it("pulls the end back to the cap from the start", () => {
    const clamped = clampWindow({ from: FROM, to: "2027-10-18T00:00:00.000Z" });
    const days = (new Date(clamped.to).getTime() - new Date(clamped.from).getTime()) / 86_400_000;
    expect(days).toBe(MAX_WINDOW_DAYS);
    expect(clamped.from).toBe(FROM);
  });

  it("honours a custom cap and passes unparseable bounds through", () => {
    const clamped = clampWindow({ from: FROM, to: "2026-10-25T00:00:00.000Z" }, 1);
    expect(clamped.to).toBe("2026-10-19T00:00:00.000Z");
    const broken = { from: "not-a-date", to: TO };
    expect(clampWindow(broken)).toBe(broken);
  });
});

describe("occurrencesQueryOptions", () => {
  it("reads through the contract with the clamped window", async () => {
    const payload = { occurrences: [], has_more: true, next_window_start: null };
    contracts.listSchedulerOccurrences.mockResolvedValue(payload);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const options = occurrencesQueryOptions(
      // The query function only forwards the client; a stub is enough.
      {} as never,
      SLUG,
      PROJECT,
      { from: FROM, to: "2027-10-18T00:00:00.000Z" }
    );
    await expect(options.queryFn?.({} as never)).resolves.toBe(payload);
    expect(contracts.listSchedulerOccurrences).toHaveBeenCalledTimes(1);
    const [client, slug, projectId, window] = contracts.listSchedulerOccurrences.mock.calls[0] as [
      unknown,
      string,
      string,
      { from: string; to: string },
    ];
    expect(client).toEqual({});
    expect(slug).toBe(SLUG);
    expect(projectId).toBe(PROJECT);
    expect(window.to).not.toBe("2027-10-18T00:00:00.000Z");
    queryClient.clear();
  });

  it("surfaces occurrences and the truncation flag through the hook", async () => {
    contracts.listSchedulerOccurrences.mockResolvedValue({
      occurrences: [
        {
          binding_id: "b1",
          scheduler_id: "s1",
          scheduler_name: "Weekly",
          scheduler_color: "#5e6ad2",
          dtstart: FROM,
          tzid: "UTC",
          kind: "scheduled",
          agent_run_id: null,
          status: null,
        },
      ],
      has_more: true,
      next_window_start: null,
    });
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useSchedulerOccurrences(SLUG, PROJECT, { from: FROM, to: TO }), {
      wrapper: wrapper(queryClient),
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.has_more).toBe(true);
    expect(result.current.data?.occurrences).toHaveLength(1);
    expect(contracts.listSchedulerOccurrences).toHaveBeenCalledWith(expect.anything(), SLUG, PROJECT, {
      from: FROM,
      to: TO,
    });
  });
});
