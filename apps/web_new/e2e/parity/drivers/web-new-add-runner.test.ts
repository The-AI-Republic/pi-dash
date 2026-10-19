// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver add-runner methods (NEWFRONT-543,
// RUN-006-009). A stub Page/Locator stands in for Playwright; the methods
// use dialog roles, named picker buttons, option lists and route spies.
import type { Locator, Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";
import { WebNewDriver } from "./web-new";

function stubLocator(overrides: Record<string, unknown> = {}) {
  const locator: Record<string, unknown> = {
    waitFor: vi.fn(async () => undefined),
    count: vi.fn(async () => 0),
    first: () => locator,
    last: () => locator,
    innerText: vi.fn(async () => ""),
    textContent: vi.fn(async () => ""),
    allInnerTexts: vi.fn(async () => []),
    getAttribute: vi.fn(async () => null),
    click: vi.fn(async () => undefined),
    fill: vi.fn(async () => undefined),
    inputValue: vi.fn(async () => ""),
    isDisabled: vi.fn(async () => false),
    isVisible: vi.fn(async () => false),
    locator: vi.fn(() => locator),
    getByRole: vi.fn(() => locator),
    getByText: vi.fn(() => locator),
    filter: vi.fn(() => locator),
    or: vi.fn(() => locator),
    ...overrides,
  };
  return locator as unknown as Locator & Record<string, ReturnType<typeof vi.fn> | unknown>;
}

function pageStub(overrides: Record<string, unknown> = {}): Page {
  return {
    goto: vi.fn(async () => null),
    url: () => "https://app.test/acme/runners",
    waitForTimeout: vi.fn(async () => undefined),
    getByRole: vi.fn(() => stubLocator()),
    getByText: vi.fn(() => stubLocator()),
    locator: vi.fn(() => stubLocator()),
    keyboard: { press: vi.fn(async () => undefined) },
    context: () => ({ grantPermissions: vi.fn(async () => undefined) }),
    evaluate: vi.fn(async () => ""),
    route: vi.fn(async (_pattern: string, _handler: unknown) => undefined),
    unroute: vi.fn(async (_pattern: string) => undefined),
    ...overrides,
  } as unknown as Page;
}

/** A dialog whose picker buttons and fields answer from one table. */
function dialogStub(overrides: Record<string, unknown> = {}) {
  return stubLocator({
    getByRole: vi.fn((role: string) => stubLocator({ count: vi.fn(async () => (role === "dialog" ? 1 : 0)) })),
    getByText: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    ...overrides,
  });
}

describe("addRunner entry points", () => {
  it("opens the workspace runners page and clicks Add runner", async () => {
    const add = stubLocator({ count: vi.fn(async () => 1) });
    const dialog = dialogStub();
    const page = pageStub({
      getByRole: vi.fn((role: string) => (role === "dialog" ? dialog : add)),
    });
    const driver = new WebNewDriver(page);

    await driver.addRunnerOpenFromRunners("acme");

    expect(page.goto).toHaveBeenCalledWith("/acme/runners", { timeout: 60_000 });
    expect(add.click).toHaveBeenCalled();
  });

  it("opens the project runners page when a project id is given", async () => {
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 1) })) });
    await new WebNewDriver(page).addRunnerOpenFromRunners("acme", "p-1");
    expect(page.goto).toHaveBeenCalledWith("/acme/projects/p-1/runners", { timeout: 60_000 });
  });

  it("opens the dev-machines page", async () => {
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 1) })) });
    await new WebNewDriver(page).addRunnerOpenFromMachines("acme");
    expect(page.goto).toHaveBeenCalledWith("/acme/ai-dev-machines", { timeout: 60_000 });
  });
});

describe("addRunnerVisible and layout", () => {
  it("is false without a dialog", async () => {
    const page = pageStub({ getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })) });
    await expect(new WebNewDriver(page).addRunnerVisible()).resolves.toBe(false);
  });

  it("reads the form, command and remote layouts", async () => {
    const form = dialogStub({
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 1) })),
    });
    const command = dialogStub({
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 1) })),
    });
    const remote = dialogStub({
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });

    await expect(new WebNewDriver(pageStub({ getByRole: () => form })).addRunnerLayout()).resolves.toBe("form");
    await expect(new WebNewDriver(pageStub({ getByRole: () => command })).addRunnerLayout()).resolves.toBe("command");
    await expect(new WebNewDriver(pageStub({ getByRole: () => remote })).addRunnerLayout()).resolves.toBe("remote");
  });
});

describe("addRunnerForm", () => {
  it("reads every picker and both inputs", async () => {
    const text: Record<string, string> = {
      "Dev machine": "Run `pidash runner add` manually",
      Project: "Acme",
      "Pod (optional)": "(default pod)",
      Agent: "Claude Code",
      "Model (optional)": "Opus 4.8",
    };
    const dialog = stubLocator({
      getByRole: vi.fn((_role: string, options?: { name?: string }) =>
        stubLocator({
          innerText: vi.fn(async () => text[options?.name ?? ""] ?? ""),
          isDisabled: vi.fn(async () => true),
        })
      ),
      locator: vi.fn((selector: string) =>
        stubLocator({ inputValue: vi.fn(async () => (selector === "#add-runner-name" ? "my-runner" : "/work")) })
      ),
    });
    const driver = new WebNewDriver(pageStub({ getByRole: () => dialog }));

    await expect(driver.addRunnerForm()).resolves.toEqual({
      machine: "Run `pidash runner add` manually",
      project: "Acme",
      projectLocked: true,
      pod: "(default pod)",
      name: "my-runner",
      workingDir: "/work",
      agent: "Claude Code",
      model: "Opus 4.8",
    });
  });
});

describe("addRunner option reads", () => {
  it("opens each named picker and returns trimmed labels", async () => {
    const option = stubLocator({ count: vi.fn(async () => 1) });
    const close = stubLocator({ waitFor: vi.fn(async () => undefined) });
    const options = stubLocator({
      first: () => option,
      allInnerTexts: vi.fn(async () => [" Opus 5.5 ", "Opus 5"]),
    });
    const trigger = stubLocator({ count: vi.fn(async () => 1) });
    const dialog = stubLocator({
      getByRole: vi.fn(() => trigger),
    });
    const page = pageStub({
      getByRole: vi.fn((role: string) => (role === "option" ? options : role === "dialog" ? dialog : close)),
    });
    const driver = new WebNewDriver(page);

    await expect(driver.addRunnerModelOptions()).resolves.toEqual(["Opus 5.5", "Opus 5"]);
    expect(trigger.click).toHaveBeenCalled();
  });

  it("surfaces project and name validation text", async () => {
    const project = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "Pick a project.") });
    const name = stubLocator({
      count: vi.fn(async () => 1),
      innerText: vi.fn(async () => "Runner name cannot contain spaces."),
    });
    const dialog = stubLocator({
      getByRole: vi.fn(() => project),
      getByText: vi.fn(() => name),
    });
    const driver = new WebNewDriver(pageStub({ getByRole: () => dialog }));

    await expect(driver.addRunnerProjectError()).resolves.toBe("Pick a project.");
    await expect(driver.addRunnerNameError()).resolves.toBe("Runner name cannot contain spaces.");
  });

  it("returns null when no error renders", async () => {
    const dialog = dialogStub();
    const driver = new WebNewDriver(pageStub({ getByRole: () => dialog }));
    await expect(driver.addRunnerProjectError()).resolves.toBeNull();
    await expect(driver.addRunnerNameError()).resolves.toBeNull();
  });
});

describe("addRunner picks and inputs", () => {
  it("skips the machine portal when it already shows the target", async () => {
    const trigger = stubLocator({ innerText: vi.fn(async () => "Laptop") });
    const dialog = stubLocator({ getByRole: vi.fn(() => trigger) });
    const page = pageStub({ getByRole: () => dialog });

    await new WebNewDriver(page).addRunnerPickMachine("Laptop");

    expect(trigger.click).not.toHaveBeenCalled();
  });

  it("fills the name and working directory fields", async () => {
    const name = stubLocator();
    const working = stubLocator();
    const dialog = stubLocator({
      locator: vi.fn((selector: string) => (selector === "#add-runner-name" ? name : working)),
    });
    const driver = new WebNewDriver(pageStub({ getByRole: () => dialog }));

    await driver.addRunnerSetName("my-runner");
    await driver.addRunnerSetWorkingDir("/tmp/work");

    expect(name.fill).toHaveBeenCalledWith("my-runner", { timeout: 30_000 });
    expect(working.fill).toHaveBeenCalledWith("/tmp/work", { timeout: 30_000 });
  });
});

describe("addRunner close and phases", () => {
  it("steps back out of an error panel before closing", async () => {
    const back = stubLocator({ count: vi.fn(async () => 1) });
    const cancel = stubLocator({ count: vi.fn(async () => 1) });
    const dialog = stubLocator({
      innerText: vi.fn(async () => "Runner creation failed: boom"),
      getByRole: vi.fn((_role: string, options?: { name?: string }) => {
        if (options?.name === "Generate Runner") return stubLocator({ count: vi.fn(async () => 0) });
        return options?.name === "Back" ? back : cancel;
      }),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });
    const driver = new WebNewDriver(pageStub({ getByRole: () => dialog }));

    await driver.addRunnerClose();

    expect(back.click).toHaveBeenCalled();
    expect(cancel.click).toHaveBeenCalled();
  });

  it("throws when the modal offers no dismiss control", async () => {
    const dialog = dialogStub({
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
      locator: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
    });
    const driver = new WebNewDriver(pageStub({ getByRole: () => dialog }));

    await expect(driver.addRunnerClose()).rejects.toThrow(/no dismiss control/);
  });

  it("maps remote phases and reads the created name", async () => {
    const phase = (body: string) => {
      const dialog = stubLocator({
        innerText: vi.fn(async () => body),
        getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 0) })),
        locator: vi.fn((selector: string) =>
          selector === "pre"
            ? stubLocator({ count: vi.fn(async () => 0) })
            : stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "new-runner") })
        ),
      });
      return new WebNewDriver(pageStub({ getByRole: () => dialog }));
    };

    await expect(phase("Runner created").addRunnerRemotePhase()).resolves.toBe("ok");
    await expect(phase("Runner creation failed: boom").addRunnerRemotePhase()).resolves.toBe("error");
    await expect(phase("did not report back in time").addRunnerRemotePhase()).resolves.toBe("timeout");
    await expect(phase("Waiting for the dev machine").addRunnerRemotePhase()).resolves.toBe("creating");
    await expect(phase("Runner created").addRunnerRemoteRunnerName()).resolves.toBe("new-runner");
    await expect(phase("Runner created").addRunnerRemoteText()).resolves.toBe("Runner created");
  });

  it("returns null for remote reads outside the remote layout", async () => {
    const dialog = dialogStub({
      getByRole: vi.fn(() => stubLocator({ count: vi.fn(async () => 1) })),
    });
    const driver = new WebNewDriver(pageStub({ getByRole: () => dialog }));
    await expect(driver.addRunnerRemotePhase()).resolves.toBeNull();
    await expect(driver.addRunnerRemoteText()).resolves.toBeNull();
    await expect(driver.addRunnerRemoteRunnerName()).resolves.toBeNull();
  });
});

describe("addRunner command panel", () => {
  function commandPage(preText = "pidash runner add --project ACME") {
    const pressed = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "PowerShell") });
    const shell = stubLocator({
      allInnerTexts: vi.fn(async () => ["POSIX", "PowerShell", "Command Prompt"]),
      locator: vi.fn(() => pressed),
    });
    const pre = stubLocator({ count: vi.fn(async () => 1), textContent: vi.fn(async () => preText) });
    const copy = stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "Copy command") });
    const dialog = stubLocator({
      getByRole: vi.fn((role: string, options?: { name?: string }) => {
        if (role === "group") return shell;
        if (role === "button" && options?.name instanceof RegExp) return copy;
        if (role === "button" && options?.name === "Generate Runner")
          return stubLocator({ count: vi.fn(async () => 0) });
        if (role === "button") return stubLocator({ count: vi.fn(async () => 1) });
        return stubLocator({ count: vi.fn(async () => 0) });
      }),
      locator: vi.fn((selector: string) =>
        selector === "pre"
          ? pre
          : selector.includes("aria-pressed")
            ? stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "PowerShell") })
            : stubLocator()
      ),
      getByText: vi.fn(() =>
        stubLocator({ count: vi.fn(async () => 1), innerText: vi.fn(async () => "Using the current browser origin") })
      ),
    });
    return { dialog, page: pageStub({ getByRole: () => dialog }), copy, pre };
  }

  it("reads the command, its project header, shells and active shell", async () => {
    const { page } = commandPage();
    const driver = new WebNewDriver(page);

    await expect(driver.addRunnerCommandText()).resolves.toBe("pidash runner add --project ACME");
    await expect(driver.addRunnerCommandHeader()).resolves.toBe("Using the current browser origin");
    await expect(driver.addRunnerShellOptions()).resolves.toEqual(["POSIX", "PowerShell", "Command Prompt"]);
    await expect(driver.addRunnerActiveShell()).resolves.toBe("PowerShell");
  });

  it("grants clipboard permission, reads copy state, clipboard and origin note", async () => {
    const { page, copy } = commandPage();
    const driver = new WebNewDriver(page);

    await driver.addRunnerCopy();
    await expect(driver.addRunnerCopyState()).resolves.toBe("Copy command");
    await expect(driver.addRunnerReadClipboard()).resolves.toBe("");
    await expect(driver.addRunnerOriginNote()).resolves.toBe("Using the current browser origin");
    expect(copy.click).toHaveBeenCalled();
    expect(page.evaluate).toHaveBeenCalled();
  });

  it("patches Clipboard.writeText to reject when breaking the clipboard", async () => {
    const { page } = commandPage();
    await new WebNewDriver(page).addRunnerBreakClipboard();
    expect(page.evaluate).toHaveBeenCalledWith(expect.any(Function));
  });
});

describe("addRunner spies", () => {
  function request(method: string, url: string, body = "") {
    return { method: () => method, url: () => url, postData: () => body };
  }

  it("records create posts and status reads through one route", async () => {
    let handler: ((route: Route) => Promise<void>) | null = null;
    const page = pageStub({
      route: vi.fn(async (_pattern: string, next: (route: Route) => Promise<void>) => {
        handler = next;
      }),
    });
    const driver = new WebNewDriver(page);

    await driver.addRunnerCreateSpyStart();
    await driver.addRunnerStatusSpyStart();
    expect(page.route).toHaveBeenCalledTimes(1);
    expect(handler).not.toBeNull();

    const route = (method: string, url: string, body = "") =>
      ({
        request: () => request(method, url, body),
        continue: vi.fn(async () => undefined),
      }) as unknown as Route;

    await handler!(route("POST", "https://api.test/api/runners/dev-machines/m-1/create-runner/", '{"project":"ACME"}'));
    await handler!(route("GET", "https://api.test/api/runners/dev-machines/m-1/create-runner/req-1/"));
    await handler!(route("GET", "https://api.test/api/runners/dev-machines/"));

    await expect(driver.addRunnerCreateSpyBodies()).resolves.toEqual(['{"project":"ACME"}']);
    await expect(driver.addRunnerStatusSpyUrls()).resolves.toEqual([
      "https://api.test/api/runners/dev-machines/m-1/create-runner/req-1/",
    ]);

    await driver.addRunnerCreateSpyStop();
    expect(page.unroute).not.toHaveBeenCalled();
    await driver.addRunnerStatusSpyStop();
    expect(page.unroute).toHaveBeenCalledWith("**/api/runners/dev-machines/**");
  });

  it("reports the last toast text or null", async () => {
    const toast = stubLocator({
      count: vi.fn(async () => 1),
      textContent: vi.fn(async () => " Error! Could not copy "),
    });
    const page = pageStub({ locator: () => toast });
    await expect(new WebNewDriver(page).addRunnerLastToast()).resolves.toBe("Error! Could not copy");

    const none = pageStub({ locator: () => stubLocator({ count: vi.fn(async () => 0) }) });
    await expect(new WebNewDriver(none).addRunnerLastToast()).resolves.toBeNull();
  });
});
