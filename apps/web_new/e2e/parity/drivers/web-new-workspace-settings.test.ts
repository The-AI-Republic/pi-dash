// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver workspace-settings general methods
// (NEWFRONT-535, WSET-003–006). A stub Page stands in for Playwright; the
// methods use goto, waitForLoadState, waitForFunction, url, getByTestId,
// getByRole, locator, count, isVisible, innerText, inputValue, fill, click,
// setInputFiles, waitFor, waitForResponse, route/unrouteAll, evaluate and
// context().grantPermissions.
import type { Page, Route } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";
import { WebNewDriver } from "./web-new";

interface LocatorStub {
  count?: () => Promise<number>;
  isVisible?: () => Promise<boolean>;
  isEnabled?: () => Promise<boolean>;
  isDisabled?: () => Promise<boolean>;
  innerText?: () => Promise<string>;
  inputValue?: () => Promise<string>;
  fill?: (value: string, options?: unknown) => Promise<void>;
  click?: (options?: unknown) => Promise<void>;
  setInputFiles?: (path: string, options?: unknown) => Promise<void>;
  waitFor?: (options?: unknown) => Promise<void>;
  first?: () => LocatorStub;
  getAttribute?: (name: string) => Promise<string | null>;
  filter?: (options: unknown) => LocatorStub;
  nth?: (index: number) => LocatorStub;
  locator?: (selector: string) => LocatorStub;
  hasText?: unknown;
}

function element(overrides: LocatorStub = {}): LocatorStub {
  const self: LocatorStub = {
    count: async () => 1,
    isVisible: async () => true,
    isEnabled: async () => true,
    isDisabled: async () => false,
    innerText: async () => "",
    inputValue: async () => "",
    fill: async () => undefined,
    click: async () => undefined,
    waitFor: async () => undefined,
    first: () => self,
    filter: () => self,
    nth: () => self,
    ...overrides,
  };
  return self;
}

function stubPage(overrides: Record<string, unknown> = {}): Page {
  const page = {
    goto: vi.fn(async () => undefined),
    waitForLoadState: vi.fn(async () => undefined),
    waitForFunction: vi.fn(async () => undefined),
    waitForTimeout: vi.fn(async () => undefined),
    waitForURL: vi.fn(async () => undefined),
    waitForResponse: vi.fn(async () => undefined),
    url: () => "http://localhost:3000/acme/settings/",
    evaluate: vi.fn(async () => "http://localhost:3000/acme"),
    getByTestId: () => element(),
    getByRole: () => element(),
    getByText: () => element(),
    locator: () => element(),
    context: () => ({ grantPermissions: vi.fn(async () => undefined) }),
    ...overrides,
  };
  return page as unknown as Page;
}

describe("wsSettingsOpen", () => {
  it("navigates to the section address and waits for a settled surface", async () => {
    const page = stubPage();
    const driver = new WebNewDriver(page);
    await driver.wsSettingsOpen("acme", "members");
    expect(page.goto as ReturnType<typeof vi.fn>).toHaveBeenCalledWith("/acme/settings/members/", { timeout: 60_000 });
    expect(page.waitForLoadState as ReturnType<typeof vi.fn>).toHaveBeenCalledWith("domcontentloaded");
    expect(page.waitForFunction as ReturnType<typeof vi.fn>).toHaveBeenCalled();
    expect(await driver.wsSettingsPath()).toBe("/acme/settings/");
  });

  it("opens the general address without a suffix", async () => {
    const page = stubPage();
    const driver = new WebNewDriver(page);
    await driver.wsSettingsOpen("acme", "");
    expect(page.goto as ReturnType<typeof vi.fn>).toHaveBeenCalledWith("/acme/settings/", { timeout: 60_000 });
  });
});

describe("general field probes", () => {
  it("reads the form visibility and the name field", async () => {
    const fill = vi.fn(async () => undefined);
    const inputValue = vi.fn(async () => "Acme");
    const page = stubPage({ locator: () => element({ fill, inputValue }) });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsGeneralVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsNameValue()).resolves.toBe("Acme");
    await driver.wsSettingsFillName("Renamed");
    expect(fill).toHaveBeenCalledWith("Renamed", { timeout: 15_000 });
  });

  it("reports a missing form and a missing name error", async () => {
    const page = stubPage({
      getByTestId: () => element({ count: async () => 0 }),
      locator: () => element({ count: async () => 0 }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsGeneralVisible()).resolves.toBe(false);
    await expect(driver.wsSettingsNameError()).resolves.toBeNull();
  });

  it("reads the inline name error text", async () => {
    const page = stubPage({ locator: () => element({ innerText: async () => " Workspace name is required " }) });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsNameError()).resolves.toBe("Workspace name is required");
  });

  it("reads the URL field and the disabled flag", async () => {
    const page = stubPage({
      locator: () =>
        element({
          inputValue: async () => "http://localhost:3000/acme",
          isDisabled: async () => true,
        }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsUrlValue()).resolves.toBe("http://localhost:3000/acme");
    await expect(driver.wsSettingsNameDisabled()).resolves.toBe(true);
  });
});

describe("size and timezone pickers", () => {
  it("lists the size presets and closes on the name field", async () => {
    const optionTexts = ["Just myself", "2-10", "11-50", "51-200", "201-500", "500+"];
    const click = vi.fn(async () => undefined);
    const options = element({
      count: async () => optionTexts.length,
      nth: (index) => element({ innerText: async () => optionTexts[index] ?? "" }),
    });
    const page = stubPage({
      getByRole: () => element({ innerText: async () => "2-10", click }),
      locator: (selector: string) => (selector === "[role='option']" ? options : element({ click })),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsSizeOptions()).resolves.toEqual(optionTexts);
    expect(click).toHaveBeenCalled();
  });

  it("picks a size by its label", async () => {
    const click = vi.fn(async () => undefined);
    const page = stubPage({ getByRole: () => element({ click }) });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsSelectSize("11-50");
    expect(click).toHaveBeenCalled();
  });

  it("searches the timezone list and picks the matching option", async () => {
    const fill = vi.fn(async () => undefined);
    const click = vi.fn(async () => undefined);
    const page = stubPage({
      getByRole: (role: string) =>
        role === "combobox" ? element({ fill }) : element({ innerText: async () => "UTC", click }),
      locator: (selector: string) =>
        selector === "h4"
          ? element({
              locator: () => element({ innerText: async () => "UTC", click }),
            })
          : element({ click }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsTimezoneLabel()).resolves.toBe("UTC");
    await driver.wsSettingsSelectTimezone("Auckland", "Auckland, Wellington");
    expect(fill).toHaveBeenCalledWith("Auckland", { timeout: 15_000 });
    expect(click).toHaveBeenCalled();
  });
});

describe("save state", () => {
  it("reads the visible label and settles after a click", async () => {
    const click = vi.fn(async () => undefined);
    const page = stubPage({ getByRole: () => element({ innerText: async () => "Updating", click }) });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsSaveVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsSaveLabel()).resolves.toBe("Updating");
    await driver.wsSettingsSave();
    expect(click).toHaveBeenCalled();
  });

  it("reports no save control for members", async () => {
    const page = stubPage({ getByRole: () => element({ count: async () => 0 }) });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsSaveVisible()).resolves.toBe(false);
    await expect(driver.wsSettingsSaveLabel()).resolves.toBeNull();
  });

  it("holds only the workspace PATCH and passes other routes through", async () => {
    let handler!: (route: Route) => Promise<void>;
    const route = vi.fn(async (_pattern: string, installed: (route: Route) => Promise<void>) => {
      handler = installed;
    });
    const page = stubPage({ route });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsDelaySave(1);
    expect(route).toHaveBeenCalledWith("**/api/workspaces/**", expect.any(Function));
    const patch = {
      request: () => ({ method: () => "PATCH", url: () => "http://localhost:3000/api/workspaces/acme/" }),
      continue: vi.fn(async () => undefined),
      fallback: vi.fn(async () => undefined),
    };
    await handler(patch as unknown as Route);
    expect(patch.continue).toHaveBeenCalled();
    const other = {
      request: () => ({ method: () => "GET", url: () => "http://localhost:3000/api/workspaces/beta/" }),
      continue: vi.fn(async () => undefined),
      fallback: vi.fn(async () => undefined),
    };
    await handler(other as unknown as Route);
    expect(other.fallback).toHaveBeenCalled();
  });

  it("fails only the workspace PATCH and passes other routes through", async () => {
    let handler!: (route: Route) => Promise<void>;
    const route = vi.fn(async (_pattern: string, installed: (route: Route) => Promise<void>) => {
      handler = installed;
    });
    const page = stubPage({ route });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsFailSave();
    const patch = {
      request: () => ({ method: () => "PATCH", url: () => "not a url" }),
      continue: vi.fn(async () => undefined),
      fallback: vi.fn(async () => undefined),
      fulfill: vi.fn(async () => undefined),
    };
    await handler(patch as unknown as Route);
    expect(patch.fallback).toHaveBeenCalled();
    const real = {
      request: () => ({ method: () => "PATCH", url: () => "http://localhost:3000/api/workspaces/acme/" }),
      continue: vi.fn(async () => undefined),
      fallback: vi.fn(async () => undefined),
      fulfill: vi.fn(async () => undefined),
    };
    await handler(real as unknown as Route);
    expect(real.fulfill).toHaveBeenCalledWith(expect.objectContaining({ status: 500 }));
  });

  it("clears interception routes", async () => {
    const unrouteAll = vi.fn(async () => undefined);
    const driver = new WebNewDriver(stubPage({ unrouteAll }));
    await driver.wsSettingsClearSaveRoutes();
    expect(unrouteAll).toHaveBeenCalledWith({ behavior: "wait" });
  });
});

describe("logo probes", () => {
  it("reports fallback vs image and the offered action", async () => {
    let count = 0;
    const page = stubPage({
      locator: () => element({ count: async () => count, getAttribute: async () => "/logo.png" }),
      getByRole: (_role: string, options?: { name?: string }) =>
        element({ count: async () => (options?.name === "Upload logo" ? 1 : 0) }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsLogoState()).resolves.toBe("fallback");
    await expect(driver.wsSettingsLogoSrc()).resolves.toBeNull();
    await expect(driver.wsSettingsLogoAction()).resolves.toBe("upload");
    count = 1;
    await expect(driver.wsSettingsLogoState()).resolves.toBe("image");
    await expect(driver.wsSettingsLogoSrc()).resolves.toBe("/logo.png");
  });

  it("reports edit and none actions", async () => {
    const page = stubPage({
      getByRole: (_role: string, options?: { name?: string }) =>
        element({ count: async () => (options?.name === "Edit logo" ? 1 : 0) }),
    });
    await expect(new WebNewDriver(page).wsSettingsLogoAction()).resolves.toBe("edit");
    const none = stubPage({ getByRole: () => element({ count: async () => 0 }) });
    await expect(new WebNewDriver(none).wsSettingsLogoAction()).resolves.toBe("none");
  });

  it("refuses to open the dialog when no action is offered", async () => {
    const page = stubPage({ getByRole: () => element({ count: async () => 0 }) });
    await expect(new WebNewDriver(page).wsSettingsOpenLogoDialog()).rejects.toThrow("no logo action");
  });

  it("opens the dialog, uploads, removes and closes", async () => {
    const setInputFiles = vi.fn(async () => undefined);
    const click = vi.fn(async () => undefined);
    const waitFor = vi.fn(async () => undefined);
    const page = stubPage({
      locator: () => element({ setInputFiles, waitFor, count: async () => 1, isVisible: async () => true }),
      getByRole: (_role: string, options?: { name?: RegExp | string }) =>
        element({ click, count: async () => (options?.name === "Upload logo" ? 1 : 0) }),
      waitForResponse: vi.fn(async () => undefined),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsLogoDialogVisible()).resolves.toBe(true);
    await driver.wsSettingsOpenLogoDialog();
    await driver.wsSettingsUploadLogo("/tmp/logo.png");
    expect(setInputFiles).toHaveBeenCalledWith("/tmp/logo.png", { timeout: 15_000 });
    await driver.wsSettingsRemoveLogo();
    await driver.wsSettingsCloseLogoDialog();
    expect(click).toHaveBeenCalled();
  });
});

describe("copy and delete probes", () => {
  it("grants clipboard permission, clicks the copy control and reads back", async () => {
    const grantPermissions = vi.fn(async () => undefined);
    const evaluate = vi.fn(async () => "http://localhost:3000/acme");
    const page = stubPage({
      context: () => ({ grantPermissions }),
      getByRole: () => element(),
      evaluate,
    });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsCopyUrl();
    await expect(driver.wsSettingsReadClipboard()).resolves.toBe("http://localhost:3000/acme");
    expect(grantPermissions).toHaveBeenCalledWith(["clipboard-read", "clipboard-write"]);
  });

  it("reads the dialog text and the confirm state, then confirms", async () => {
    const waitForFunction = vi.fn(async () => undefined);
    const page = stubPage({
      waitForFunction,
      getByText: () => element({ isVisible: async () => true }),
      locator: (selector: string) =>
        selector === "form" ? element({ innerText: async () => "Acme delete my workspace" }) : element(),
      getByRole: () => element({ isEnabled: async () => true }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.wsSettingsDeleteSectionVisible()).resolves.toBe(true);
    await driver.wsSettingsOpenDeleteDialog();
    await expect(driver.wsSettingsDeleteDialogVisible()).resolves.toBe(true);
    await expect(driver.wsSettingsDeleteDialogText()).resolves.toBe("Acme delete my workspace");
    await expect(driver.wsSettingsConfirmDeleteEnabled()).resolves.toBe(true);
    await driver.wsSettingsConfirmDelete();
    expect(waitForFunction).toHaveBeenCalled();
  });

  it("fills both confirmation fields and cancels", async () => {
    const fill = vi.fn(async () => undefined);
    const page = stubPage({ locator: () => element({ fill }) });
    const driver = new WebNewDriver(page);
    await driver.wsSettingsFillDeleteName("Acme");
    await driver.wsSettingsFillDeletePhrase("delete my workspace");
    await driver.wsSettingsCancelDelete();
    expect(fill).toHaveBeenCalledWith("Acme", { timeout: 15_000 });
    expect(fill).toHaveBeenCalledWith("delete my workspace", { timeout: 15_000 });
  });
});

describe("rulesLastToast", () => {
  it("reads the newest visible card as title plus message", async () => {
    const cards = [
      element({ innerText: async () => "Success!\nWorkspace updated successfully" }),
      element({ innerText: async () => "Older notice" }),
    ];
    const locator = vi.fn(() => ({
      ...element(),
      count: async () => cards.length,
      nth: (index: number) => cards[index] as LocatorStub,
    }));
    const page = stubPage({ locator });
    const driver = new WebNewDriver(page);
    await expect(driver.rulesLastToast()).resolves.toEqual({
      title: "Success!",
      message: "Workspace updated successfully",
    });
    expect(locator).toHaveBeenCalledWith('[data-testid="toast-notification"]');
  });

  it("skips hidden and blank cards and reports nothing when none is visible", async () => {
    const cards = [
      element({ isVisible: async () => false, innerText: async () => "Hidden" }),
      element({ innerText: async () => "   " }),
    ];
    const locator = vi.fn(() => ({
      ...element(),
      count: async () => cards.length,
      nth: (index: number) => cards[index] as LocatorStub,
    }));
    const driver = new WebNewDriver(stubPage({ locator }));
    await expect(driver.rulesLastToast()).resolves.toBeNull();
  });

  it("returns null when the stack holds no cards", async () => {
    const locator = vi.fn(() => element({ count: async () => 0 }));
    const driver = new WebNewDriver(stubPage({ locator }));
    await expect(driver.rulesLastToast()).resolves.toBeNull();
  });
});

describe("rulesEnsureSignedIn", () => {
  it("signs out of a live session, then signs in and settles on the requested workspace", async () => {
    let url = "http://localhost:3000/beta/settings/";
    const page = stubPage({
      url: () => url,
      waitForURL: vi.fn(async () => undefined),
    }) as Page & Record<string, unknown>;
    const driver = new WebNewDriver(page);
    const signOut = vi.spyOn(driver, "signOutViaCommandPalette").mockImplementation(async () => {
      url = "http://localhost:3000/";
    });
    const openEntry = vi.spyOn(driver, "openEntry").mockImplementation(async () => undefined);
    const signIn = vi.spyOn(driver, "signInWithPassword").mockImplementation(async () => {
      url = "http://localhost:3000/acme/home/";
    });
    await driver.rulesEnsureSignedIn("ann@example.com", "secret", "acme");
    expect(signOut).toHaveBeenCalledTimes(1);
    expect(openEntry).toHaveBeenCalledTimes(1);
    expect(signIn).toHaveBeenCalledWith("ann@example.com", "secret");
    expect(page.waitForURL).toHaveBeenCalledWith(/\/acme(\/|$)/, { timeout: 60_000 });
  });

  it("skips the sign-out when already signed out", async () => {
    const page = stubPage({ getByPlaceholder: () => element({ isVisible: async () => true }) });
    const driver = new WebNewDriver(page);
    const signOut = vi.spyOn(driver, "signOutViaCommandPalette").mockImplementation(async () => undefined);
    vi.spyOn(driver, "openEntry").mockImplementation(async () => undefined);
    vi.spyOn(driver, "signInWithPassword").mockImplementation(async () => undefined);
    await driver.rulesEnsureSignedIn("ann@example.com", "secret", "acme");
    expect(signOut).not.toHaveBeenCalled();
  });

  it("lands on the workspace home when sign-in does not reach it", async () => {
    let url = "http://localhost:3000/";
    const page = stubPage({
      url: () => url,
      goto: vi.fn(async (target: string) => {
        url = `http://localhost:3000${target}`;
      }),
      waitForURL: vi.fn(async () => {
        throw new Error("timeout");
      }),
    });
    const driver = new WebNewDriver(page);
    vi.spyOn(driver, "signOutViaCommandPalette").mockImplementation(async () => undefined);
    vi.spyOn(driver, "openEntry").mockImplementation(async () => undefined);
    vi.spyOn(driver, "signInWithPassword").mockImplementation(async () => undefined);
    await driver.rulesEnsureSignedIn("ann@example.com", "secret", "acme");
    expect(page.goto).toHaveBeenCalledWith("/acme/", { timeout: 60_000 });
    expect(page.waitForLoadState).toHaveBeenCalledWith("domcontentloaded");
  });
});
