// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The analytics dialog driver reads run against real DOM here, not a
// scripted answer: the title read must survive the panel's several `h2`
// headings (ANL-014), and the assignee facts must read the name label
// past the avatar's initials glyph (ANL-015). Each browser-serialized
// predicate is captured from the driver and invoked in this document, so
// the test fails when the shipped selector stops matching the markup.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { Locator, Page } from "@playwright/test";
import type { AnalyticsProjectStats } from "@pidash/api-client";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createAppClient, resetAppClient } from "../../../src/core/api/client.js";

const { contractMocks } = vi.hoisted(() => ({
  contractMocks: { getProjectAnalyticsStats: vi.fn() },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contractMocks };
});

import { AssigneeBreakdownSection } from "../../../src/features/analytics/components/AssigneeBreakdownSection.js";
import { WebNewDriver } from "./web-new.js";

/** A page that runs each evaluated predicate inside this jsdom document. */
function evaluatingPage(): Page {
  return { evaluate: async (fn: () => unknown) => fn() } as unknown as Page;
}

/**
 * The dialog title read as Playwright resolves it: the dialog's `h2`
 * locator is strict, so it throws while the settled panel has more than
 * one heading; `first()` pins a single node first. This is the panel's
 * shape after the scope resolves, so the read has to pin before waiting.
 */
function titledDialogPage(title: string, extraHeadings: number): Page {
  const headings = (): NodeListOf<HTMLHeadingElement> =>
    document.querySelectorAll<HTMLHeadingElement>('[role="dialog"] h2');
  const strict = (): never => {
    throw new Error(`strict mode violation: locator resolved to ${headings().length} elements`);
  };
  const firstNode = (): Locator =>
    ({
      waitFor: async () => {
        if (headings().length === 0) throw new Error("element not found");
      },
      textContent: async () => headings()[0]?.textContent ?? "",
    }) as unknown as Locator;
  const allNodes = (): Locator =>
    ({
      first: firstNode,
      waitFor: async () => {
        if (headings().length !== 1) strict();
      },
      textContent: async () => {
        if (headings().length !== 1) strict();
        return headings()[0]?.textContent ?? "";
      },
    }) as unknown as Locator;
  const dialog = (): Locator =>
    ({ locator: (selector: string) => (selector === "h2" ? allNodes() : dialog()) }) as unknown as Locator;
  const page = { getByRole: () => dialog() } as unknown as Page;
  document.body.innerHTML = `
    <div role="dialog">
      <h2>${title}</h2>
      ${Array.from({ length: extraHeadings }, (_, index) => `<section><h2>Section ${index + 1}</h2></section>`).join("")}
    </div>`;
  return page;
}

function statsRow(extra: Partial<AnalyticsProjectStats[number]> = {}): AnalyticsProjectStats[number] {
  return {
    display_name: "Ada Lovelace",
    assignee_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    avatar_url: "https://cdn.test/ada.png",
    cancelled_work_items: 0,
    completed_work_items: 1,
    backlog_work_items: 2,
    un_started_work_items: 3,
    started_work_items: 4,
    ...extra,
  };
}

function renderAssignees(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <AssigneeBreakdownSection
        workspaceSlug="acme"
        scope={{ projectId: "223e4567-e89b-12d3-a456-426614174001", cycleId: null, moduleId: null }}
      />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  createAppClient("https://api.test", () => new Promise<never>(() => {}));
});

afterEach(() => {
  resetAppClient();
  document.body.innerHTML = "";
});

describe("analytics dialog driver against the rendered panel", () => {
  it("reads the dialog title past the section headings", async () => {
    const page = titledDialogPage("Analytics for Website in Alpha", 3);
    await expect(new WebNewDriver(page).analyticsDialogTitle()).resolves.toBe("Analytics for Website in Alpha");
  });

  it("reads each assignee's name, not its initials glyph", async () => {
    contractMocks.getProjectAnalyticsStats.mockResolvedValue([
      statsRow({ display_name: "Ada Lovelace" }),
      statsRow({ display_name: "Zoe", avatar_url: null }),
      statsRow({ display_name: null, assignee_id: null, avatar_url: null }),
    ]);
    renderAssignees();
    await vi.waitFor(() => {
      expect(document.querySelectorAll('[data-testid="analytics-dialog-assignee-table"] tbody tr')).toHaveLength(3);
    });
    const facts = await new WebNewDriver(evaluatingPage()).analyticsDialogAssigneeFacts();
    expect(facts).toEqual([
      { text: "Ada Lovelace", avatar: true, initial: false, unassigned: false },
      { text: "Zoe", avatar: false, initial: true, unassigned: false },
      { text: "Unassigned", avatar: false, initial: false, unassigned: true },
    ]);
  });
});
