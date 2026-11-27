// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the old app's draft row-menu clicker (DRAFT-009, DRAFT-025).
// Scenarios name row entries by the draft actions catalog ids; since
// main#532 the old app renders the translated labels, so the driver must
// map an id to its label and let an already-labelled call through. A
// recording fake stands in for Playwright and keeps every locator chain
// call, so a test can assert the accessible name the click targeted.
import type { Page } from "@playwright/test";
import { describe, expect, it } from "vitest";

import { WebDriver } from "./web.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["filter", "first", "last", "nth", "getByRole", "getByText", "locator"] as const;
const ACTIONS = ["count", "isVisible", "click", "hover", "waitFor", "scrollIntoViewIfNeeded"] as const;

/** A page where every locator answers one visible row with one visible trigger. */
function recorder(): { page: Page; calls: Call[] } {
  const calls: Call[] = [];
  const nodeAt = (path: string): Record<string, unknown> => {
    const node: Record<string, unknown> = {};
    for (const method of CHAIN) {
      node[method] = (...args: unknown[]) => {
        calls.push({ path, method, args });
        return nodeAt(`${path}.${method}`);
      };
    }
    for (const method of ACTIONS) {
      node[method] = async (...args: unknown[]) => {
        calls.push({ path, method, args });
        if (method === "count") return 1;
        if (method === "isVisible") return true;
        return undefined;
      };
    }
    return node;
  };
  const page = {
    locator: () => nodeAt("page.locator"),
    getByText: () => nodeAt("page.getByText"),
  };
  return { page: page as unknown as Page, calls };
}

/** The accessible name of the role lookup whose menu entry the driver clicked. */
function clickedName(calls: Call[]): unknown {
  const named = calls.filter((call) => {
    if (call.method !== "getByRole") return false;
    const options = call.args[1] as { name?: unknown } | undefined;
    return options?.name !== undefined;
  });
  return (named.at(-1)?.args[1] as { name?: unknown } | undefined)?.name;
}

describe("old-app draft row menu clicker", () => {
  it.each([
    ["edit", "Edit"],
    ["make_a_copy", "Make a copy"],
    ["move_to_project", "Move to project"],
    ["delete", "Delete"],
  ])("maps the %s catalog id onto the label the old app renders", async (entry, label) => {
    const { page, calls } = recorder();
    await new WebDriver(page).draftRowMenuClick("One", entry);
    expect(clickedName(calls)).toBe(label);
    // The click follows the translation: the menu entry was opened, not a raw key.
    expect(calls.some((call) => call.method === "click" && call.path.endsWith("filter.nth"))).toBe(true);
  });

  it("passes an already-labelled entry through untouched", async () => {
    const { page, calls } = recorder();
    await new WebDriver(page).draftRowMenuClick("One", "Make a copy");
    expect(clickedName(calls)).toBe("Make a copy");
  });

  it("passes an uncatalogued entry through so the scenario's own failure names it", async () => {
    const { page, calls } = recorder();
    await new WebDriver(page).draftRowMenuClick("One", "Send to inbox");
    expect(clickedName(calls)).toBe("Send to inbox");
  });
});
