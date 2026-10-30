// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Regression tests for WebNewDriver.awaitAppBoot and the crash watch
// (NEWFRONT-1031; same shape as NEWFRONT-808 batches 12/13). On noisy
// hosts Chromium intermittently aborts the vite dev server's route-module
// fetches, so the error boundary renders "Failed to fetch dynamically
// imported module" instead of the page. The boot check reloads while that
// vite signature shows (up to five loads) and leaves every other state —
// content, app errors, slow loads — for the scenario's own waits. A stub
// Page scripts paint, body text, and reloads; no browser runs here.
import type { Page } from "@playwright/test";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebNewDriver } from "./web-new";

const CHUNK_ERROR = "Something went wrong! Failed to fetch dynamically imported module: http://x/y.tsx";
const APP_ERROR = "Something went wrong! Could not load module details";
const CONTENT = "Parity Workspace Modules Skeleton Module";

afterEach(() => {
  vi.restoreAllMocks();
});

function silenceLogs(): void {
  vi.spyOn(console, "log").mockImplementation(() => {});
}

interface ScriptedBootPage {
  holds: number;
  reloads: number;
  loads: number;
  page: Page;
}

/** A stub page with scripted paint and per-load body text. */
function scriptedBootPage(options: { painted?: boolean; bodies?: string[]; unreadable?: boolean }): ScriptedBootPage {
  const state = { holds: 0, reloads: 0, loads: 0 };
  const bodies = options.bodies ?? [CONTENT];
  const page = {
    waitForFunction: async () => {
      if (options.painted === false) throw new Error("timeout");
    },
    waitForTimeout: async () => {
      state.holds += 1;
    },
    evaluate: async () => {
      if (options.unreadable === true) throw new Error("detached");
      return bodies[Math.min(state.loads, bodies.length - 1)];
    },
    reload: async () => {
      state.reloads += 1;
      state.loads += 1;
    },
    waitForLoadState: async () => {},
  };
  return Object.assign(state, { page: page as unknown as Page });
}

/** The private boot check exposed for focused tests (same trick as web-favorites). */
function bootOf(driver: WebNewDriver): (path: string) => Promise<void> {
  return (driver as unknown as { awaitAppBoot: (path: string) => Promise<void> }).awaitAppBoot.bind(driver);
}

describe("awaitAppBoot", () => {
  it("returns without reloading on a healthy load", async () => {
    silenceLogs();
    const script = scriptedBootPage({});
    const driver = new WebNewDriver(script.page);
    await bootOf(driver)("/ws/projects/p/modules");
    expect(script.holds).toBe(1);
    expect(script.reloads).toBe(0);
    expect(console.log).not.toHaveBeenCalled();
  });

  it("reloads once past a transient chunk error and logs the recovery", async () => {
    silenceLogs();
    const script = scriptedBootPage({ bodies: [CHUNK_ERROR, CONTENT] });
    const driver = new WebNewDriver(script.page);
    await bootOf(driver)("/ws/projects/p/modules");
    expect(script.reloads).toBe(1);
    expect(script.holds).toBe(2);
    expect(console.log).toHaveBeenCalledWith(
      "[parity] vite chunk error at /ws/projects/p/modules (load 1/5); reloading."
    );
  });

  it("throws after five loads of a persistent chunk error", async () => {
    silenceLogs();
    const script = scriptedBootPage({ bodies: [CHUNK_ERROR] });
    const driver = new WebNewDriver(script.page);
    await expect(bootOf(driver)("/ws/projects/p/modules")).rejects.toThrow(
      "[parity] vite chunk error persists at /ws/projects/p/modules after 5 loads."
    );
    expect(script.reloads).toBe(4);
    expect(console.log).toHaveBeenCalledTimes(5);
  });

  it("leaves a blank stall for the scenario's own waits", async () => {
    silenceLogs();
    const script = scriptedBootPage({ painted: false });
    const driver = new WebNewDriver(script.page);
    await bootOf(driver)("/ws/projects/p/modules");
    expect(script.holds).toBe(0);
    expect(script.reloads).toBe(0);
  });

  it("never reloads an app error without the vite signature", async () => {
    silenceLogs();
    const script = scriptedBootPage({ bodies: [APP_ERROR] });
    const driver = new WebNewDriver(script.page);
    await bootOf(driver)("/ws/projects/p/modules");
    expect(script.reloads).toBe(0);
  });

  it("treats an unreadable body as booted", async () => {
    silenceLogs();
    const script = scriptedBootPage({ bodies: [CHUNK_ERROR], unreadable: true });
    const driver = new WebNewDriver(script.page);
    await bootOf(driver)("/ws/projects/p/modules");
    expect(script.reloads).toBe(0);
  });

  it.each(["waitForFunction", "waitForTimeout", "evaluate", "reload"] as const)(
    "skips the check when the page lacks %s",
    async (missing) => {
      silenceLogs();
      const script = scriptedBootPage({ bodies: [CHUNK_ERROR, CONTENT] });
      const page = { ...(script.page as unknown as Record<string, unknown>), [missing]: undefined };
      const driver = new WebNewDriver(page as unknown as Page);
      await bootOf(driver)("/ws/projects/p/modules");
      expect(script.holds).toBe(0);
      expect(script.reloads).toBe(0);
    }
  );
});

interface ScriptedNavPage {
  holds: number;
  reloads: number;
  gotos: number;
  page: Page;
}

/** A stub page that navigates instantly and settles every locator wait. */
function scriptedNavPage(options: { url?: string; bodies?: string[] }): ScriptedNavPage {
  const state = { holds: 0, reloads: 0, gotos: 0, loads: 0 };
  const bodies = options.bodies ?? [CONTENT];
  const settled = { last: () => ({ waitFor: async () => {} }) };
  const page = {
    url: () => options.url ?? "http://localhost:3080/ws",
    goto: async () => {
      state.gotos += 1;
    },
    waitForLoadState: async () => {},
    waitForFunction: async () => {},
    waitForTimeout: async () => {
      state.holds += 1;
    },
    evaluate: async () => bodies[Math.min(state.loads, bodies.length - 1)],
    reload: async () => {
      state.reloads += 1;
      state.loads += 1;
    },
    locator: () => settled,
    context: () => ({ addCookies: async () => {} }),
  };
  return Object.assign(state, { page: page as unknown as Page });
}

describe("navigation boot wiring", () => {
  it("openAuthenticated recovers past a first-load chunk error", async () => {
    silenceLogs();
    const script = scriptedNavPage({ bodies: [CHUNK_ERROR, CONTENT] });
    const driver = new WebNewDriver(script.page);
    await driver.openAuthenticated("/ws/projects/p/modules", []);
    expect(script.reloads).toBe(1);
    expect(script.holds).toBe(2);
  });

  it("modulesXOpenModuleDetail recovers past a first-load chunk error", async () => {
    silenceLogs();
    const script = scriptedNavPage({ bodies: [CHUNK_ERROR, CONTENT] });
    const driver = new WebNewDriver(script.page);
    await driver.modulesXOpenModuleDetail("ws", "p", "m");
    expect(script.gotos).toBe(1);
    expect(script.reloads).toBe(1);
  });

  it("modulesXOpenModulesList recovers past a first-load chunk error", async () => {
    silenceLogs();
    const script = scriptedNavPage({ bodies: [CHUNK_ERROR, CONTENT] });
    const driver = new WebNewDriver(script.page);
    await driver.modulesXOpenModulesList("ws", "p");
    expect(script.gotos).toBe(1);
    expect(script.reloads).toBe(1);
  });

  it("modulesXOpenModulesListWithPeek recovers past a first-load chunk error", async () => {
    silenceLogs();
    const script = scriptedNavPage({ bodies: [CHUNK_ERROR, CONTENT] });
    const driver = new WebNewDriver(script.page);
    await driver.modulesXOpenModulesListWithPeek("ws", "p", "m");
    expect(script.gotos).toBe(1);
    expect(script.reloads).toBe(1);
  });

  it("modulesListOpenList recovers when it navigates", async () => {
    silenceLogs();
    const script = scriptedNavPage({ bodies: [CHUNK_ERROR, CONTENT] });
    const driver = new WebNewDriver(script.page);
    await driver.modulesListOpenList("ws", "p");
    expect(script.gotos).toBe(1);
    expect(script.reloads).toBe(1);
  });

  it("modulesListOpenList skips the check when already on the list", async () => {
    silenceLogs();
    const script = scriptedNavPage({ url: "http://localhost:3080/ws/projects/p/modules", bodies: [CHUNK_ERROR] });
    const driver = new WebNewDriver(script.page);
    await driver.modulesListOpenList("ws", "p");
    expect(script.gotos).toBe(0);
    expect(script.holds).toBe(0);
    expect(script.reloads).toBe(0);
  });
});

describe("crash watch", () => {
  /** A full boot-surface stub with a recording event surface. */
  function crashPage(): { script: ScriptedBootPage; handlers: Map<string, () => void>; page: Page } {
    const script = scriptedBootPage({});
    const handlers = new Map<string, () => void>();
    const page = {
      ...(script.page as unknown as Record<string, unknown>),
      url: () => "http://localhost:3010/ws/projects/p/modules",
      on: (event: string, handler: () => void) => {
        handlers.set(event, handler);
      },
    };
    return { script, handlers, page: page as unknown as Page };
  }

  it("logs a renderer crash with its last URL", async () => {
    silenceLogs();
    const { handlers, page } = crashPage();
    const driver = new WebNewDriver(page);
    await bootOf(driver)("/ws/projects/p/modules");
    expect([...handlers.keys()]).toEqual(["crash"]);
    handlers.get("crash")?.();
    expect(console.log).toHaveBeenCalledWith("[parity] page crashed at http://localhost:3010/ws/projects/p/modules.");
  });

  it("arms once no matter how many navigations run", async () => {
    silenceLogs();
    const { handlers, page } = crashPage();
    let calls = 0;
    const counting = {
      ...(page as unknown as Record<string, unknown>),
      on: (event: string, handler: () => void) => {
        calls += 1;
        handlers.set(event, handler);
      },
    };
    const driver = new WebNewDriver(counting as unknown as Page);
    await bootOf(driver)("/a");
    await bootOf(driver)("/b");
    expect(calls).toBe(1);
  });

  it("runs the boot check on pages without an event surface", async () => {
    silenceLogs();
    const script = scriptedBootPage({ bodies: [CHUNK_ERROR, CONTENT] });
    const driver = new WebNewDriver(script.page);
    await bootOf(driver)("/ws/projects/p/modules");
    expect(script.reloads).toBe(1);
  });
});
