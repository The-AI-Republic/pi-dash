// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Executes the page-side predicates of the workspace-settings general driver
// (NEWFRONT-535) against a real DOM: the settle probe after navigation, the
// idle-label probe after saving, and the PATCH matcher after a logo removal.
import type { Page, Response } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebNewDriver } from "./web-new";

type Fn = (...args: never[]) => unknown;

function capturePage(overrides: Record<string, unknown> = {}): Page {
  return {
    goto: vi.fn(async () => undefined),
    waitForLoadState: vi.fn(async () => undefined),
    waitForFunction: vi.fn(async () => undefined),
    waitForTimeout: vi.fn(async () => undefined),
    waitForResponse: vi.fn(async () => undefined),
    url: () => "http://localhost:3000/acme/settings/",
    getByRole: () => ({
      first: () => ({ click: vi.fn(async () => undefined) }),
      click: vi.fn(async () => undefined),
    }),
    locator: () => ({ first: () => ({ waitFor: vi.fn(async () => undefined) }) }),
    ...overrides,
  } as unknown as Page;
}

function predicates(page: Page, method: "waitForFunction" | "waitForResponse"): Fn[] {
  const mock = page[method] as unknown as ReturnType<typeof vi.fn>;
  return mock.mock.calls.map((call) => call[0] as Fn);
}

function rect(width: number): DOMRect {
  return { width, height: 10, top: 0, left: 0, right: width, bottom: 10, x: 0, y: 0, toJSON: () => ({}) } as DOMRect;
}

afterEach(() => {
  document.title = "";
  document.body.innerHTML = "";
});

describe("wsSettingsOpen settle probe", () => {
  it("settles on a titled document", async () => {
    document.title = "General";
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsOpen("acme", "general");
    const [settled] = predicates(page, "waitForFunction");
    expect(settled()).toBe(true);
  });

  it("settles on the shell placeholder", async () => {
    document.body.innerHTML = "<div data-testid='workspace-settings-section-placeholder'><span>x</span></div>";
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsOpen("acme", "general");
    expect(predicates(page, "waitForFunction")[0]()).toBe(true);
  });

  it("settles on a visible denial message and ignores it when collapsed", async () => {
    document.body.innerHTML = "<p>You are not authorized</p>";
    const node = document.querySelector("p") as HTMLElement;
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsOpen("acme", "general");
    const [settled] = predicates(page, "waitForFunction");

    node.getBoundingClientRect = () => rect(0);
    expect(settled()).toBe(false);
    node.getBoundingClientRect = () => rect(120);
    expect(settled()).toBe(true);
  });

  it("does not settle on an unrelated visible node", async () => {
    document.body.innerHTML = "<div><p>Loading</p></div>";
    const node = document.querySelector("p") as HTMLElement;
    node.getBoundingClientRect = () => rect(120);
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsOpen("acme", "general");
    expect(predicates(page, "waitForFunction")[0]()).toBe(false);
  });
});

describe("wsSettingsSave idle probe", () => {
  it("waits for the idle Update workspace label", async () => {
    // jsdom has no innerText; the probe reads it from real browsers.
    document.body.innerHTML = "<button></button>";
    const button = document.querySelector("button") as HTMLElement;
    const label = { value: "  Update Workspace " };
    Object.defineProperty(button, "innerText", { get: () => label.value, configurable: true });
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsSave();
    const [idle] = predicates(page, "waitForFunction");
    expect(idle()).toBe(true);

    label.value = "Updating…";
    expect(idle()).toBe(false);
    label.value = undefined as unknown as string;
    expect(idle()).toBe(false);
  });

  it("does not settle when no button exists", async () => {
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsSave();
    expect(predicates(page, "waitForFunction")[0]()).toBe(false);
  });
});

describe("wsSettingsRemoveLogo PATCH matcher", () => {
  it("matches only the workspace PATCH", async () => {
    const page = capturePage();
    await new WebNewDriver(page).wsSettingsRemoveLogo();
    const [matches] = predicates(page, "waitForResponse");
    const response = (method: string, url: string) =>
      ({ request: () => ({ method: () => method, url: () => url }) }) as unknown as Response;

    expect(matches(response("PATCH", "http://localhost:3000/api/workspaces/acme/") as never)).toBe(true);
    expect(matches(response("POST", "http://localhost:3000/api/workspaces/acme/") as never)).toBe(false);
    expect(matches(response("PATCH", "http://localhost:3000/api/workspaces/beta/") as never)).toBe(false);
    expect(matches(response("PATCH", "not a url") as never)).toBe(false);
  });
});

describe("wsSettingsSizeLabel", () => {
  it("returns the collapsed label", async () => {
    const page = capturePage({
      getByRole: () => ({ first: () => ({ innerText: async () => "  51-200\n workspaces " }) }),
    });
    await expect(new WebNewDriver(page).wsSettingsSizeLabel()).resolves.toBe("51-200 workspaces");
  });

  it("returns an empty label when the control is missing or empty", async () => {
    const missing = capturePage({
      getByRole: () => ({
        first: () => ({
          innerText: async () => {
            throw new Error("detached");
          },
        }),
      }),
    });
    await expect(new WebNewDriver(missing).wsSettingsSizeLabel()).resolves.toBe("");

    const blank = capturePage({ getByRole: () => ({ first: () => ({ innerText: async () => null }) }) });
    await expect(new WebNewDriver(blank).wsSettingsSizeLabel()).resolves.toBe("");
  });
});
