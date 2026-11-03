// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver cloud assistant/profile methods (area
// gate NEWFRONT-662, CLOUD-032–040). A stub Page stands in for
// Playwright: the assertions pin the selectors and branches against
// the cloud-assistant components — never a real browser.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

interface StubLocator {
  count?: () => Promise<number>;
  click?: (options?: unknown) => Promise<void>;
  innerText?: () => Promise<string>;
  first?: () => StubLocator;
  [key: string]: unknown;
}

function locator(overrides: Partial<StubLocator> = {}): Locator {
  const node: StubLocator = {
    count: async () => 0,
    click: async () => undefined,
    innerText: async () => "",
    allInnerTexts: async () => [],
    isVisible: async () => false,
    isDisabled: async () => false,
    isEnabled: async () => false,
    isChecked: async () => false,
    inputValue: async () => "",
    fill: async () => undefined,
    press: async () => undefined,
    selectOption: async () => undefined,
    getAttribute: async () => null,
    waitFor: async () => undefined,
    ...overrides,
  };
  const chain: Record<string, unknown> = {
    first: () => node,
    nth: () => locator(),
    locator: () => locator(),
    getByRole: () => locator(),
    getByTestId: () => locator(),
    getByPlaceholder: () => locator(),
    getByLabel: () => locator(),
    getByText: () => locator(),
    filter: () => locator(),
    or: () => locator(),
  };
  for (const [name, value] of Object.entries(chain)) {
    if (node[name] === undefined) node[name] = value;
  }
  return node as unknown as Locator;
}

function pageOf(overrides: Partial<Record<string, unknown>> = {}): Page {
  return {
    getByRole: vi.fn(() => locator()),
    getByTestId: vi.fn(() => locator()),
    getByPlaceholder: vi.fn(() => locator()),
    getByLabel: vi.fn(() => locator()),
    getByText: vi.fn(() => locator()),
    locator: vi.fn(() => locator()),
    ...overrides,
  } as unknown as Page;
}

interface ConnFacts {
  title: string;
  markers?: string[];
  checked?: boolean;
  disabled?: boolean;
  control?: "select" | "input" | "hidden";
  modelValue?: string;
  modelOptions?: string[];
  verified?: boolean;
  actions?: string[];
  onSelect?: () => void;
  onAction?: (action: string) => void;
  filled?: Array<[string, string]>;
  staleNotice?: string | null;
}

function connCard(facts: ConnFacts): Locator {
  const label = locator({
    locator: (selector: string) => {
      if (selector === "span") return locator({ allInnerTexts: async () => [facts.title, ...(facts.markers ?? [])] });
      if (selector === "xpath=parent::div")
        return locator({ getByRole: () => locator({ allInnerTexts: async () => facts.actions ?? [] }) });
      return locator();
    },
  });
  const radioState = { checked: facts.checked ?? false };
  return locator({
    count: async () => 1,
    locator: (selector: string) => {
      if (selector === "label") return label;
      if (selector === 'input[name="assistant-active-connection"]')
        return locator({
          isChecked: async () => radioState.checked,
          isDisabled: async () => facts.disabled ?? false,
          click: async () => {
            radioState.checked = true;
            facts.onSelect?.();
          },
        });
      if (selector === "select") {
        if (facts.control !== "select") return locator();
        return locator({
          count: async () => 1,
          isVisible: async () => true,
          inputValue: async () => facts.modelValue ?? "",
          locator: (inner: string) => {
            if (inner !== "option") return locator();
            const options = facts.modelOptions ?? [];
            return locator({
              count: async () => options.length,
              nth: (index: number) => locator({ getAttribute: async () => options[index] ?? "" }),
            });
          },
          selectOption: async (value: unknown) => void facts.filled?.push(["select", String(value)]),
        });
      }
      if (selector === 'input[list="assistant-known-byok-models"]') {
        if (facts.control !== "input") return locator();
        return locator({
          count: async () => 1,
          isVisible: async () => true,
          inputValue: async () => facts.modelValue ?? "",
          fill: async (value: string) => void facts.filled?.push(["input", value]),
          press: async (key: string) => void facts.filled?.push(["press", key]),
        });
      }
      return locator();
    },
    getByText: (text: string) => {
      if (typeof text === "string" && text.includes("Verified"))
        return locator({
          count: async () => (facts.verified === true ? 1 : 0),
          isVisible: async () => facts.verified === true,
        });
      if (typeof text === "string" && text.includes("predates OpenHub access")) {
        if (facts.staleNotice === undefined || facts.staleNotice === null) return locator();
        return locator({
          count: async () => 1,
          isVisible: async () => true,
          locator: () => locator({ innerText: async () => facts.staleNotice ?? "" }),
        });
      }
      return locator();
    },
    getByRole: (role: string, options?: { name?: string }) => {
      if (role !== "button") return locator();
      return locator({
        click: async () => void facts.onAction?.(options?.name ?? "?"),
      });
    },
  });
}

function assistantPage(entries: Locator[]): Page {
  return pageOf({
    locator: (selector: string) => {
      if (selector === '[data-testid^="assistant-connection-"]')
        return locator({ count: async () => entries.length, nth: (index: number) => entries[index] ?? locator() });
      return locator();
    },
  });
}

describe("cloud assistant connections", () => {
  it("reads titles, markers, selection and model controls per card", async () => {
    const driver = new WebNewDriver(
      assistantPage([
        connCard({
          title: "Hosted",
          markers: ["Active"],
          checked: true,
          control: "select",
          modelValue: "sonnet",
          modelOptions: ["sonnet", "opus"],
          verified: true,
          actions: ["Test"],
        }),
        connCard({ title: "Custom endpoint", control: "input", modelValue: "llama", actions: ["Test", "Remove"] }),
        connCard({ title: "ChatGPT account", control: "hidden", disabled: true }),
      ])
    );
    await expect(driver.cloudAssistantConnections()).resolves.toEqual([
      {
        title: "Hosted",
        selected: true,
        selectDisabled: false,
        markers: ["Active"],
        modelControl: "select",
        modelValue: "sonnet",
        modelOptions: ["sonnet", "opus"],
        verifiedVisible: true,
        actions: ["Test"],
      },
      {
        title: "Custom endpoint",
        selected: false,
        selectDisabled: false,
        markers: [],
        modelControl: "input",
        modelValue: "llama",
        modelOptions: [],
        verifiedVisible: false,
        actions: ["Test", "Remove"],
      },
      {
        title: "ChatGPT account",
        selected: false,
        selectDisabled: true,
        markers: [],
        modelControl: "hidden",
        modelValue: "",
        modelOptions: [],
        verifiedVisible: false,
        actions: [],
      },
    ]);
  });

  it("selects by title and waits for the radio to check", async () => {
    let selected = "";
    const driver = new WebNewDriver(
      assistantPage([connCard({ title: "One" }), connCard({ title: "Two", onSelect: () => void (selected = "Two") })])
    );
    await driver.cloudAssistantSelectConnection("Two");
    expect(selected).toBe("Two");
  });

  it("sets the model through the select or the free-text field", async () => {
    const filled: Array<[string, string]> = [];
    const driver = new WebNewDriver(
      assistantPage([
        connCard({ title: "Hosted", control: "select", filled }),
        connCard({ title: "Custom endpoint", control: "input", filled }),
      ])
    );
    await driver.cloudAssistantSetModel("Hosted", "opus");
    await driver.cloudAssistantSetModel("Custom endpoint", "llama-3");
    expect(filled).toEqual([
      ["select", "opus"],
      ["input", "llama-3"],
      ["press", "Enter"],
    ]);
  });

  it("fires row actions and confirms removals inline", async () => {
    const acted: string[] = [];
    const driver = new WebNewDriver(
      assistantPage([connCard({ title: "Custom endpoint", onAction: (action) => void acted.push(action) })])
    );
    await driver.cloudAssistantClickRowAction("Custom endpoint", "Test");
    await driver.cloudAssistantClickRowAction("Custom endpoint", "Remove");
    expect(acted).toEqual(["Test", "Remove", "Confirm remove"]);
  });

  it("reads the stale-session notice from the named card", async () => {
    const driver = new WebNewDriver(
      assistantPage([
        connCard({ title: "OpenHub", staleNotice: "This session predates OpenHub access. Sign out and back in." }),
        connCard({ title: "Hosted" }),
      ])
    );
    await expect(driver.cloudAssistantStaleNotice("OpenHub")).resolves.toContain("predates OpenHub access");
    await expect(driver.cloudAssistantStaleNotice("Hosted")).resolves.toBeNull();
  });
});

describe("cloud assistant views and add form", () => {
  function viewPage(view: "list" | "add", saveEnabled = false): Page {
    const clicked: string[] = [];
    return pageOf({
      getByRole: (role: string, options?: { name?: string }) => {
        if (role === "button" && options?.name === "Back to connections")
          return locator({ count: async () => (view === "add" ? 1 : 0), isVisible: async () => view === "add" });
        if (role === "button" && options?.name === "Add connection")
          return locator({
            count: async () => 1,
            isEnabled: async () => saveEnabled,
            click: async () => void clicked.push("add"),
          });
        return locator();
      },
      getByLabel: (label: string) => {
        if (label === "Provider")
          return locator({
            inputValue: async () => "openai-compatible",
            locator: () =>
              locator({
                count: async () => 2,
                nth: (index: number) =>
                  locator({ getAttribute: async () => ["openai-compatible", "anthropic"][index] ?? "" }),
              }),
            selectOption: async (value: unknown) => void clicked.push(`provider:${String(value)}`),
          });
        if (label === "Base URL") return locator({ count: async () => 1, isVisible: async () => true });
        return locator();
      },
    });
  }

  it("distinguishes the list view from the add view", async () => {
    await expect(new WebNewDriver(viewPage("list")).cloudAssistantView()).resolves.toBe("list");
    await expect(new WebNewDriver(viewPage("add")).cloudAssistantView()).resolves.toBe("add");
  });

  it("reads the add-form providers, base visibility and save state", async () => {
    const driver = new WebNewDriver(viewPage("add", true));
    await expect(driver.cloudAssistantAddFormFacts()).resolves.toEqual({
      providers: ["openai-compatible", "anthropic"],
      provider: "openai-compatible",
      baseVisible: true,
      saveEnabled: true,
    });
  });

  it("reports the empty state with its add label", async () => {
    const withEmpty = pageOf({
      getByText: () =>
        locator({
          count: async () => 1,
          isVisible: async () => true,
          locator: () =>
            locator({ getByRole: () => locator({ count: async () => 1, innerText: async () => "Add connection" }) }),
        }),
    });
    await expect(new WebNewDriver(withEmpty).cloudAssistantEmptyState()).resolves.toEqual({
      visible: true,
      addLabel: "Add connection",
    });
    await expect(new WebNewDriver(pageOf()).cloudAssistantEmptyState()).resolves.toEqual({
      visible: false,
      addLabel: null,
    });
  });
});

describe("cloud profile tabs, sections and badge", () => {
  it("collects tab labels from links and buttons without duplicates", async () => {
    const page = pageOf({
      getByRole: () => locator({ allInnerTexts: async () => ["Save", "AI Assistant"] }),
      locator: (selector: string) => {
        if (selector === '[data-testid^="profile-settings-tab-"]')
          return locator({ allInnerTexts: async () => ["Profile", "AI Assistant"] });
        return locator();
      },
    });
    await expect(new WebNewDriver(page).cloudProfileSidebarTabs()).resolves.toEqual(["AI Assistant", "Profile"]);
  });

  it("reads the active tab from the new marker, else the old one", async () => {
    const fresh = pageOf({
      locator: (selector: string) => {
        if (selector === '[data-testid^="profile-settings-tab-"][aria-current="page"]')
          return locator({ count: async () => 1, innerText: async () => "AI Assistant" });
        return locator();
      },
    });
    await expect(new WebNewDriver(fresh).cloudProfileActiveTab()).resolves.toBe("ai-assistant");
    const legacy = pageOf({
      locator: (selector: string) => {
        if (selector === "button.bg-layer-transparent-selected")
          return locator({ count: async () => 1, nth: () => locator({ innerText: async () => "Security" }) });
        return locator();
      },
    });
    await expect(new WebNewDriver(legacy).cloudProfileActiveTab()).resolves.toBe("security");
    await expect(new WebNewDriver(pageOf()).cloudProfileActiveTab()).resolves.toBeNull();
  });

  it("reads section visibility and the plan badge", async () => {
    const page = pageOf({
      getByRole: (role: string, options?: { name?: string }) =>
        role === "heading" && options?.name === "Tool servers (MCP)"
          ? locator({ count: async () => 1, isVisible: async () => true })
          : locator(),
      getByText: () => locator({ count: async () => 1, isVisible: async () => true }),
      getByTestId: (id: string) =>
        id === "cloud-plan-badge" ? locator({ count: async () => 1, isVisible: async () => true }) : locator(),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.cloudAssistantMcpSectionVisible()).resolves.toBe(true);
    await expect(driver.cloudAssistantAppsPointerVisible()).resolves.toBe(true);
    await expect(driver.cloudPlanBadgeVisible()).resolves.toBe(true);
    await expect(driver.cloudPlanBadgeFacts()).resolves.toBeNull();
  });

  it("reads the plan badge label and hint", async () => {
    const page = pageOf({
      getByTestId: (id: string) =>
        id === "cloud-plan-badge"
          ? locator({
              count: async () => 1,
              innerText: async () => "Upgrade",
              getAttribute: async (name: string) => (name === "title" ? "Version: v1.2.3" : null),
            })
          : locator(),
    });
    await expect(new WebNewDriver(page).cloudPlanBadgeFacts()).resolves.toEqual({
      label: "Upgrade",
      hint: "Version: v1.2.3",
    });
  });
});

describe("cloud assistant popups, counters and focus", () => {
  it("counts sign-in status reads after arming", async () => {
    let handler: ((request: { url: () => string }) => void) | null = null;
    const page = pageOf({
      on: vi.fn((_event: string, fn: (request: { url: () => string }) => void) => void (handler = fn)),
      off: vi.fn(),
    });
    const driver = new WebNewDriver(page);
    await driver.cloudAssistantArmStatusCounter();
    handler?.({ url: () => "https://api.test/api/users/me/ai-assistant/openai-login/status/?x=1" });
    handler?.({ url: () => "https://api.test/api/other/" });
    await expect(driver.cloudAssistantStatusHits()).resolves.toBe(1);
  });

  it("reports the focused control and blocks popups", async () => {
    const page = pageOf({
      evaluate: async () => ({ role: "button", label: "Add connection" }),
      addInitScript: vi.fn(async () => undefined),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.cloudFocusedControl()).resolves.toEqual({ role: "button", label: "Add connection" });
    await driver.cloudAssistantBlockPopups();
    expect(page.addInitScript).toHaveBeenCalled();
  });

  it("returns the provider popup URL and closes it", async () => {
    let closed = false;
    const popup = {
      waitForFunction: vi.fn(async () => undefined),
      url: () => "https://provider.example.com/consent",
      close: async () => void (closed = true),
    };
    const page = pageOf({
      context: () => ({ pages: () => [page, popup] }),
    });
    const driver = new WebNewDriver(page);
    await expect(driver.cloudAssistantProviderPopupUrl()).resolves.toBe("https://provider.example.com/consent");
    expect(closed).toBe(true);
  });
});
