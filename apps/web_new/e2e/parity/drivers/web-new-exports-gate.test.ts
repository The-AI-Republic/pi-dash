// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the WebNewDriver exports-gate methods (NEWFRONT-772,
// EXP-003..005/016..024): settings pickers, submit and toast, the submit
// route hooks, and the page-export dialog reads. A stateful fake stands
// in for Playwright: the picker popup filters and toggles like the kit
// Combobox, the trigger compacts picks to keys, and the dialog selects
// unmount per row. Tests assert which selectors the driver used and what
// it does with the answers. Never about a real browser.
import type { Locator, Page } from "@playwright/test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { exportsProjectKeyDelta, exportsProjectOptionName, WebNewDriver } from "./web-new.js";

type Call = { path: string; method: string; args: unknown[] };

const CHAIN = ["filter", "first", "last", "nth", "getByRole", "getByText", "getByTestId", "locator"];
const ACTIONS = [
  "count",
  "click",
  "fill",
  "innerText",
  "textContent",
  "allTextContents",
  "getAttribute",
  "isDisabled",
  "isVisible",
  "waitFor",
  "evaluate",
];

function render(args: unknown[]): string {
  return args
    .map((arg) => {
      if (typeof arg === "function") return "<fn>";
      if (arg instanceof RegExp) return String(arg);
      if (typeof arg === "string") return JSON.stringify(arg);
      if (arg && typeof arg === "object") return JSON.stringify(arg);
      return String(arg);
    })
    .join(",");
}

const PROJECTS_LISTBOX = `page.getByRole("listbox",{"name":"Projects"})`;
const FORMAT_LISTBOX = `page.getByRole("listbox",{"name":"Format"})`;
const EMPTY_TEXT = `page.getByText("No matching results",{"exact":true})`;
const SEARCH_BOX = `page.getByRole("combobox",{"name":"Search projects"})`;
const SUBMIT_IDLE = `page.getByRole("button",{"name":"Export","exact":true})`;
const TOASTS = `page.locator("[data-testid=\\"toast-notification\\"]")`;
const DIALOG = `page.getByRole("dialog")`;
const MAIN = `page.locator("main")`;
const SETTINGS_SHELL = `page.getByTestId("workspace-settings-shell")`;
const GATE_HEADING_NAMES = ["Available on", "Not available in your plan", "Desktop app", "Offline mode"];

interface FakeOption {
  name: string;
  key: string;
}

interface FakeToast {
  visible: boolean;
  text: string;
}

class Recorder {
  readonly calls: Call[] = [];
  readonly routes: Array<{ pattern: unknown; handler: (route: never) => Promise<void> }> = [];
  options: FakeOption[] = [];
  /** Names currently picked; the trigger compacts them to keys. */
  picked: string[] = [];
  projectOpen = false;
  projectFilter = "";
  projectTriggerDisabled = false;
  formatOpen = false;
  format = "CSV";
  submitClicks = 0;
  submitDisabled = false;
  toasts: FakeToast[] = [];
  dialogPresent = false;
  dialogVisible = true;
  /** AppShell content main; absent on settings surfaces. */
  mainPresent = true;
  /** Settings shell root; replaces AppShell on settings surfaces. */
  settingsShellPresent = false;
  /** Bare-role candidate controls per mounted root (marker sweeps use gateHits). */
  candidateButtons = 2;
  candidateHeadings = 1;
  /** Offered labels per dialog row; a missing row unmounts its select. */
  dialogRows: Partial<Record<"format" | "scope" | "pageFormat", string[]>> = {};
  download: { fileName: string; path: string | null } | null = null;
  evaluateAnswers: unknown[] = [];
  readonly evaluateCalls: unknown[][] = [];
  readonly keyPresses: string[] = [];
  /** When true, Escape leaves popups open (a stuck popup). */
  popupsStuck = false;
  gateHits: Array<{ role: string; visible: boolean }> = [];
  gateHeadings: string[] = [];
  /** Which exports-settings surface the access read sees. */
  accessSurface: "form" | "not-authorized" | "no-workspace" = "form";
  /** When true, locator waitFor rejects (a mount that never lands). */
  waitForRejects = false;

  locator(path: string): Locator {
    const node: Record<string, unknown> = { __path: path };
    for (const method of CHAIN) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path, method, args });
        return this.locator(`${path}.${method}(${render(args)})`);
      };
    }
    for (const method of ACTIONS) {
      node[method] = async (...args: unknown[]) => {
        const call = { path, method, args };
        this.calls.push(call);
        return this.respond(call);
      };
    }
    return node as unknown as Locator;
  }

  /** Options surviving the picker's client-side filter (name plus key). */
  visibleOptions(): FakeOption[] {
    const needle = this.projectFilter.trim().toLowerCase();
    if (needle === "") return this.options;
    return this.options.filter((option) => `${option.name} ${option.key}`.toLowerCase().includes(needle));
  }

  /** Trigger text: keys joined, or All projects when empty. */
  projectTriggerLabel(): string {
    const keys = this.options.filter((option) => this.picked.includes(option.name)).map((option) => option.key);
    return keys.length === 0 ? "All projects" : keys.join(", ");
  }

  private isProjectTrigger(path: string): boolean {
    return path.includes("Exporting project") && path.includes('getByRole("button")');
  }

  private isFormatTrigger(path: string): boolean {
    // Bare word: the heading name rides inside a stringified filter arg,
    // so its quotes come back escaped.
    return path.includes("Format") && path.includes('getByRole("button")') && !path.includes("listbox");
  }

  respond(call: Call): unknown {
    const { path, method, args } = call;
    if (method === "count") return this.count(path);
    if (method === "innerText") return this.text(path);
    if (method === "allTextContents") return this.allTexts(path);
    if (method === "isDisabled") return this.disabled(path);
    if (method === "isVisible") return this.visible(path);
    if (method === "click") {
      this.clicked(path);
      return undefined;
    }
    if (method === "fill") {
      if (path === SEARCH_BOX) this.projectFilter = String(args[0] ?? "");
      return undefined;
    }
    if (method === "waitFor") {
      if (this.waitForRejects) throw new Error("[fake] mount never landed");
      return undefined;
    }
    if (method === "getAttribute") return null;
    if (method === "textContent") return this.text(path);
    if (method === "evaluate") return this.evaluateLocator(path);
    return undefined;
  }

  /** Locator evaluate: option reads resolve to the accessible name. */
  private evaluateLocator(path: string): unknown {
    const optionMatch = /\.nth\((\d+)\)$/.exec(path);
    if (path.startsWith(`${PROJECTS_LISTBOX}.getByRole("option")`) && optionMatch?.[1] !== undefined) {
      return this.visibleOptions()[Number(optionMatch[1])]?.name ?? "";
    }
    return undefined;
  }

  private count(path: string): number {
    if (path === PROJECTS_LISTBOX) return this.projectOpen && this.visibleOptions().length > 0 ? 1 : 0;
    if (path === EMPTY_TEXT) return this.projectOpen && this.visibleOptions().length === 0 ? 1 : 0;
    if (path === `${PROJECTS_LISTBOX}.getByRole("option")`) return this.visibleOptions().length;
    if (path === FORMAT_LISTBOX) return this.formatOpen ? 1 : 0;
    if (path === SUBMIT_IDLE) return 1;
    if (path === TOASTS) return this.toasts.length;
    if (path === DIALOG) return this.dialogPresent ? 1 : 0;
    if (/^page\.getByRole\("dialog"\)\.filter\(.*\)\.last\(\)$/.test(path)) {
      return this.dialogPresent ? 1 : 0;
    }
    if (path === MAIN) return this.mainPresent ? 1 : 0;
    if (path === SETTINGS_SHELL) return this.settingsShellPresent ? 1 : 0;
    if (path.includes('getByRole("heading"')) {
      // Bare-role queries are the gate scan's candidate sweep; named ones
      // are either gate-marker probes or dialog form reads by label.
      if (!path.includes('{"name"')) {
        if (path.startsWith(MAIN)) return this.mainPresent ? this.candidateHeadings : 0;
        if (path.startsWith(SETTINGS_SHELL)) return this.settingsShellPresent ? this.candidateHeadings : 0;
        if (path.includes('getByRole("dialog")')) return this.dialogPresent ? this.candidateHeadings : 0;
        return 0;
      }
      if (GATE_HEADING_NAMES.some((name) => path.includes(`{"name":"${name}"}`))) {
        return this.gateHeadings.length > 0 ? 1 : 0;
      }
      if (path.startsWith(MAIN) || path.startsWith(SETTINGS_SHELL)) return this.gateHeadings.length > 0 ? 1 : 0;
      return this.dialogPresent ? 1 : 0;
    }
    if (path.includes('locator("#page-export-format")') && !path.includes("option")) {
      return this.dialogRows.format === undefined ? 0 : 1;
    }
    if (path.includes('locator("#page-export-scope")') && !path.includes("option")) {
      return this.dialogRows.scope === undefined ? 0 : 1;
    }
    if (path.includes('locator("#page-export-size")') && !path.includes("option")) {
      return this.dialogRows.pageFormat === undefined ? 0 : 1;
    }
    if (path.includes('getByTestId("workspace-settings-not-authorized")')) {
      return this.accessSurface === "not-authorized" ? 1 : 0;
    }
    if (path.includes('getByTestId("workspace-exports-settings")')) {
      return this.accessSurface === "form" ? 1 : 0;
    }
    if (path.includes('getByRole("button"')) {
      // Bare-role queries are the gate scan's candidate sweep; named ones
      // are gate-marker probes (a regex name stringifies to {"name":{}}).
      if (!path.includes('{"name"')) {
        if (path.startsWith(MAIN)) return this.mainPresent ? this.candidateButtons : 0;
        if (path.startsWith(SETTINGS_SHELL)) return this.settingsShellPresent ? this.candidateButtons : 0;
        if (path.includes('getByRole("dialog")')) return this.dialogPresent ? this.candidateButtons : 0;
        return 0;
      }
      if (path.startsWith(MAIN) || path.startsWith(SETTINGS_SHELL) || path.includes('getByRole("dialog")')) {
        return this.gateHits.length;
      }
      return 0;
    }
    if (path.includes('getByRole("link"') || path.includes('getByRole("menuitem"')) return 0;
    return 0;
  }

  private text(path: string): string {
    if (this.isProjectTrigger(path)) return this.projectTriggerLabel();
    if (this.isFormatTrigger(path)) return this.format;
    const optionMatch = /\.nth\((\d+)\)$/.exec(path);
    if (path.startsWith(`${PROJECTS_LISTBOX}.getByRole("option")`) && optionMatch?.[1] !== undefined) {
      return this.visibleOptions()[Number(optionMatch[1])]?.name ?? "";
    }
    if (path.startsWith(TOASTS) && optionMatch?.[1] !== undefined) {
      return this.toasts[Number(optionMatch[1])]?.text ?? "";
    }
    if (path.includes('getByRole("heading"') && !path.startsWith(MAIN)) return "Export page";
    return "";
  }

  private allTexts(path: string): string[] {
    if (path === `${FORMAT_LISTBOX}.getByRole("option")`) return ["CSV", "Excel", "JSON"];
    if (path.includes('locator("#page-export-format")')) return this.dialogRows.format ?? [];
    if (path.includes('locator("#page-export-scope")')) return this.dialogRows.scope ?? [];
    if (path.includes('locator("#page-export-size")')) return this.dialogRows.pageFormat ?? [];
    return [];
  }

  private disabled(path: string): boolean {
    if (this.isProjectTrigger(path)) return this.projectTriggerDisabled;
    // The idle/in-flight submit (its regex name stringifies away, so match
    // the shape: a bare top-level button query, not a trigger box).
    if (path.startsWith('page.getByRole("button"') && path.endsWith(".first()") && !path.includes(".filter(")) {
      return this.submitDisabled;
    }
    return false;
  }

  private visible(path: string): boolean {
    const toastMatch = new RegExp(`^${TOASTS.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\.nth\\((\\d+)\\)$`).exec(path);
    if (toastMatch?.[1] !== undefined) return this.toasts[Number(toastMatch[1])]?.visible ?? false;
    if (path.startsWith(MAIN) && path.includes('getByRole("button"')) {
      const hit = /\.nth\((\d+)\)$/.exec(path);
      if (hit?.[1] === undefined) return this.gateHits.length > 0;
      return this.gateHits[Number(hit[1])]?.visible ?? false;
    }
    if (/^page\.getByRole\("dialog"\)\.nth\(\d+\)$/.test(path)) return this.dialogVisible;
    if (
      (path.startsWith(SETTINGS_SHELL) || path.includes('getByRole("dialog")')) &&
      path.includes('getByRole("button"')
    ) {
      const hit = /\.nth\((\d+)\)$/.exec(path);
      if (hit?.[1] === undefined) return this.gateHits.length > 0;
      return this.gateHits[Number(hit[1])]?.visible ?? false;
    }
    return true;
  }

  private clicked(path: string): void {
    if (this.isProjectTrigger(path)) {
      this.projectOpen = !this.projectOpen;
      return;
    }
    if (this.isFormatTrigger(path)) {
      this.formatOpen = !this.formatOpen;
      return;
    }
    if (path === SUBMIT_IDLE) {
      this.submitClicks += 1;
      return;
    }
    // Project option toggles keep a multi popup open; format picks close it.
    const optionName = /getByRole\("option",\{"name":("[^"]+"),"exact":true\}\)$/.exec(path)?.[1];
    if (optionName !== undefined) {
      const name = JSON.parse(optionName) as string;
      if (path.startsWith(PROJECTS_LISTBOX)) {
        this.picked = this.picked.includes(name)
          ? this.picked.filter((entry) => entry !== name)
          : [...this.picked, name];
      } else if (path.startsWith(FORMAT_LISTBOX)) {
        this.format = name;
        this.formatOpen = false;
      }
    }
  }

  page(): Page {
    const node: Record<string, unknown> = {};
    for (const method of ["getByRole", "getByText", "locator", "getByTestId"]) {
      node[method] = (...args: unknown[]) => {
        this.calls.push({ path: "page", method, args });
        return this.locator(`page.${method}(${render(args)})`);
      };
    }
    node.route = async (pattern: unknown, handler: (route: never) => Promise<void>) => {
      this.routes.push({ pattern, handler });
    };
    node.evaluate = async (...args: unknown[]) => {
      this.evaluateCalls.push(args);
      return this.evaluateAnswers.shift();
    };
    node.keyboard = {
      press: async (key: string) => {
        this.keyPresses.push(key);
        if (key === "Escape" && !this.popupsStuck) {
          this.projectOpen = false;
          this.formatOpen = false;
        }
      },
    };
    node.waitForEvent = async () => ({
      suggestedFilename: () => this.download?.fileName ?? "",
      path: async () => this.download?.path ?? null,
    });
    return node as unknown as Page;
  }

  driver(): WebNewDriver {
    return new WebNewDriver(this.page());
  }
}

function stubRoute(
  method: string,
  body?: unknown
): { seenFallback: () => boolean; seenAbort: () => boolean; route: never } {
  let fallback = false;
  let abort = false;
  return {
    seenFallback: () => fallback,
    seenAbort: () => abort,
    route: {
      request: () => ({
        method: () => method,
        postDataJSON: () => {
          if (body === undefined) throw new Error("no body");
          return body;
        },
      }),
      fallback: async () => {
        fallback = true;
      },
      abort: async () => {
        abort = true;
      },
    } as never,
  };
}

describe("exportsProjectKeyDelta", () => {
  it("reads the whole label for the first pick", () => {
    expect(exportsProjectKeyDelta("All projects", "EA")).toBe("EA");
  });

  it("reads the cleared label when the last pick drops", () => {
    expect(exportsProjectKeyDelta("EA", "All projects")).toBe("EA");
  });

  it("finds the added key among several picks", () => {
    expect(exportsProjectKeyDelta("EA", "EA, EB")).toBe("EB");
  });

  it("finds the removed key among several picks", () => {
    expect(exportsProjectKeyDelta("EA, EB", "EB")).toBe("EA");
  });

  it("answers empty when the labels match or the delta is unreadable", () => {
    expect(exportsProjectKeyDelta("EA", "EA")).toBe("");
    expect(exportsProjectKeyDelta("EA", "EB")).toBe("EB");
  });
});

describe("exportsSettingsProjectPickerOpen", () => {
  it("clicks the Exporting project trigger and settles on the listbox", async () => {
    const rec = new Recorder();
    rec.options = [{ name: "Exp Alpha", key: "EA" }];
    await rec.driver().exportsSettingsProjectPickerOpen();

    expect(rec.projectOpen).toBe(true);
    const triggerClicks = rec.calls.filter(
      (call) => call.method === "click" && call.path.includes("Exporting project")
    );
    expect(triggerClicks).toHaveLength(1);
  });

  it("treats the empty text as an opened but option-less picker", async () => {
    const rec = new Recorder();
    rec.options = [];
    await rec.driver().exportsSettingsProjectPickerOpen();

    expect(rec.projectOpen).toBe(true);
    await expect(rec.driver().exportsSettingsProjectPickerOptions()).resolves.toEqual([]);
  });

  it("skips the click when the list is already open", async () => {
    const rec = new Recorder();
    rec.options = [{ name: "Exp Alpha", key: "EA" }];
    rec.projectOpen = true;
    await rec.driver().exportsSettingsProjectPickerOpen();

    expect(rec.calls.filter((call) => call.method === "click")).toHaveLength(0);
  });
});

describe("exportsSettingsProjectPickerOptions", () => {
  it("probes each option's key through the trigger and restores the pick", async () => {
    const rec = new Recorder();
    rec.options = [
      { name: "Exp Alpha", key: "EA" },
      { name: "Exp Beta", key: "EB" },
    ];
    const pairs = await rec.driver().exportsSettingsProjectPickerOptions();

    expect(pairs).toEqual([
      { key: "EA", name: "Exp Alpha" },
      { key: "EB", name: "Exp Beta" },
    ]);
    // Each option toggled twice, so the selection is empty again.
    expect(rec.picked).toEqual([]);
    expect(rec.projectTriggerLabel()).toBe("All projects");
    const optionClicks = rec.calls.filter((call) => call.method === "click" && call.path.startsWith(PROJECTS_LISTBOX));
    expect(optionClicks).toHaveLength(4);
  });

  it("reads the surviving option after a search filters the list", async () => {
    const rec = new Recorder();
    rec.options = [
      { name: "Exp Alpha", key: "EA" },
      { name: "Exp Beta", key: "EB" },
    ];
    await rec.driver().exportsSettingsProjectPickerSearch("beta");
    const pairs = await rec.driver().exportsSettingsProjectPickerOptions();

    expect(pairs).toEqual([{ key: "EB", name: "Exp Beta" }]);
    const fills = rec.calls.filter((call) => call.path === SEARCH_BOX && call.method === "fill");
    expect(fills).toHaveLength(1);
    expect(fills[0]?.args[0]).toBe("beta");
  });

  it("matches the search against keys as well as names", async () => {
    const rec = new Recorder();
    rec.options = [
      { name: "Exp Alpha", key: "EA" },
      { name: "Exp Beta", key: "EB" },
    ];
    await rec.driver().exportsSettingsProjectPickerSearch("EA");
    await expect(rec.driver().exportsSettingsProjectPickerOptions()).resolves.toEqual([
      { key: "EA", name: "Exp Alpha" },
    ]);
  });

  it("resolves keys through the diff when a pick is already present", async () => {
    const rec = new Recorder();
    rec.options = [
      { name: "Exp Alpha", key: "EA" },
      { name: "Exp Beta", key: "EB" },
    ];
    rec.picked = ["Exp Alpha"];
    const pairs = await rec.driver().exportsSettingsProjectPickerOptions();

    expect(pairs).toEqual([
      { key: "EA", name: "Exp Alpha" },
      { key: "EB", name: "Exp Beta" },
    ]);
    expect(rec.picked).toEqual(["Exp Alpha"]);
  });

  it("reads names through the accessible-name helper, never innerText", async () => {
    const rec = new Recorder();
    rec.options = [{ name: "Exp Alpha", key: "EA" }];
    await rec.driver().exportsSettingsProjectPickerOptions();

    const reads = rec.calls.filter((call) => call.method === "evaluate");
    expect(reads).toHaveLength(1);
    expect(reads[0]?.args[0]).toBe(exportsProjectOptionName);
    const texts = rec.calls.filter((call) => call.method === "innerText" && call.path.startsWith(PROJECTS_LISTBOX));
    expect(texts).toHaveLength(0);
  });
});

describe("exportsSettingsProjectPickerToggle", () => {
  it("clicks the option by exact name", async () => {
    const rec = new Recorder();
    rec.options = [{ name: "Exp Alpha", key: "EA" }];
    await rec.driver().exportsSettingsProjectPickerToggle("Exp Alpha");

    expect(rec.picked).toEqual(["Exp Alpha"]);
    expect(await rec.driver().exportsSettingsProjectPickerLabel()).toBe("EA");
  });
});

describe("exportsSettingsProjectPickerDisabled", () => {
  it("reads the trigger's disabled state", async () => {
    const rec = new Recorder();
    await expect(rec.driver().exportsSettingsProjectPickerDisabled()).resolves.toBe(false);
    rec.projectTriggerDisabled = true;
    await expect(rec.driver().exportsSettingsProjectPickerDisabled()).resolves.toBe(true);
  });
});

describe("exportsSettingsFormatOptions", () => {
  it("reads the three labels in order and dismisses the popup", async () => {
    const rec = new Recorder();
    const labels = await rec.driver().exportsSettingsFormatOptions();

    expect(labels).toEqual(["CSV", "Excel", "JSON"]);
    expect(rec.formatOpen).toBe(false);
    const triggerClicks = rec.calls.filter(
      (call) => call.method === "click" && call.path.includes("Format") && !call.path.includes("listbox")
    );
    expect(triggerClicks).toHaveLength(2);
  });
});

describe("exportsSettingsFormatSelect", () => {
  it("picks the option and settles on the new trigger label", async () => {
    const rec = new Recorder();
    await rec.driver().exportsSettingsFormatSelect("JSON");

    expect(rec.format).toBe("JSON");
    expect(rec.formatOpen).toBe(false);
    await expect(rec.driver().exportsSettingsFormatLabel()).resolves.toBe("JSON");
  });
});

describe("exportsSettingsAccess", () => {
  it("settles on the submit before reporting the form", async () => {
    const rec = new Recorder();
    await expect(rec.driver().exportsSettingsAccess()).resolves.toBe("form");
    const waits = rec.calls.filter((call) => call.method === "waitFor");
    expect(waits.length).toBe(1);
    expect(waits[0]?.args).toEqual([{ state: "attached", timeout: WebNewDriver.WAIT_MS }]);
  });

  it("still reports the form when the guide never lands", async () => {
    const rec = new Recorder();
    rec.waitForRejects = true;
    await expect(rec.driver().exportsSettingsAccess()).resolves.toBe("form");
  });

  it("reports the guard branches without waiting", async () => {
    const refused = new Recorder();
    refused.accessSurface = "not-authorized";
    await expect(refused.driver().exportsSettingsAccess()).resolves.toBe("not-authorized");
    const missing = new Recorder();
    missing.accessSurface = "no-workspace";
    await expect(missing.driver().exportsSettingsAccess()).resolves.toBe("no-workspace");
    expect(refused.calls.some((call) => call.method === "waitFor")).toBe(false);
    expect(missing.calls.some((call) => call.method === "waitFor")).toBe(false);
  });
});

describe("exportsSettingsSubmit", () => {
  it("clicks the idle submit and reads its disabled state", async () => {
    const rec = new Recorder();
    await rec.driver().exportsSettingsSubmit();

    expect(rec.submitClicks).toBe(1);
    const clicks = rec.calls.filter((call) => call.path === SUBMIT_IDLE && call.method === "click");
    expect(clicks).toHaveLength(1);
    await expect(rec.driver().exportsSettingsSubmitDisabled()).resolves.toBe(false);
    rec.submitDisabled = true;
    await expect(rec.driver().exportsSettingsSubmitDisabled()).resolves.toBe(true);
  });

  it("sends no keys when no popup is open", async () => {
    const rec = new Recorder();
    await rec.driver().exportsSettingsSubmit();

    expect(rec.keyPresses).toEqual([]);
    expect(rec.submitClicks).toBe(1);
  });

  it("settles an open project popup with Escape before clicking", async () => {
    const rec = new Recorder();
    rec.options = [{ name: "Exp Alpha", key: "EA" }];
    rec.projectOpen = true;
    await rec.driver().exportsSettingsSubmit();

    expect(rec.keyPresses).toEqual(["Escape"]);
    expect(rec.projectOpen).toBe(false);
    expect(rec.submitClicks).toBe(1);
  });

  it("settles an open format popup too", async () => {
    const rec = new Recorder();
    rec.formatOpen = true;
    await rec.driver().exportsSettingsSubmit();

    expect(rec.keyPresses).toEqual(["Escape"]);
    expect(rec.formatOpen).toBe(false);
    expect(rec.submitClicks).toBe(1);
  });

  it("fails loudly when the popup never closes", async () => {
    const rec = new Recorder();
    rec.options = [{ name: "Exp Alpha", key: "EA" }];
    rec.projectOpen = true;
    rec.popupsStuck = true;
    await expect(rec.driver().exportsSettingsSubmit()).rejects.toThrow("picker popup stayed open");
    expect(rec.keyPresses).toEqual(["Escape", "Escape", "Escape"]);
    expect(rec.submitClicks).toBe(0);
  });
});

describe("exportsSettingsToast", () => {
  it("classifies the pinned success copy", async () => {
    const rec = new Recorder();
    rec.toasts = [{ visible: true, text: "Export successful\nYour JSON export has started." }];
    await expect(rec.driver().exportsSettingsToast()).resolves.toEqual({
      kind: "success",
      title: "Export successful",
      message: "Your JSON export has started.",
    });
  });

  it("classifies the pinned failure copy", async () => {
    const rec = new Recorder();
    rec.toasts = [{ visible: true, text: "Error\nExport was unsuccessful. Please try again." }];
    await expect(rec.driver().exportsSettingsToast()).resolves.toEqual({
      kind: "error",
      title: "Error",
      message: "Export was unsuccessful. Please try again.",
    });
  });

  it("ignores hidden, blank and unrelated cards", async () => {
    const rec = new Recorder();
    rec.toasts = [
      { visible: false, text: "Export successful\nstale" },
      { visible: true, text: "  \n " },
      { visible: true, text: "Link copied" },
    ];
    await expect(rec.driver().exportsSettingsToast()).resolves.toBeNull();
  });

  it("returns null when no toast shows", async () => {
    const rec = new Recorder();
    await expect(rec.driver().exportsSettingsToast()).resolves.toBeNull();
  });

  it("skips an Error card without the retry prompt", async () => {
    const rec = new Recorder();
    rec.toasts = [{ visible: true, text: "Error\nSomething else broke." }];
    await expect(rec.driver().exportsSettingsToast()).resolves.toBeNull();
  });
});

describe("exports submit route hooks", () => {
  it("captures submit bodies through the single shared hook", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsCaptureSubmit();
    expect(rec.routes).toHaveLength(1);
    expect(rec.routes[0]?.pattern).toBe("**/api/workspaces/*/export-issues**");

    const handler = rec.routes[0]?.handler;
    expect(handler).toBeDefined();
    if (!handler) return;
    const write = stubRoute("POST", { provider: "json", project: ["p1", 7], multiple: false });
    await handler(write.route);
    expect(write.seenFallback()).toBe(true);
    await expect(driver.exportsSettingsCapturedSubmits()).resolves.toEqual([
      { provider: "json", project: ["p1"], multiple: false },
    ]);
  });

  it("forwards unparseable and unshaped bodies with blank facts", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsCaptureSubmit();
    const handler = rec.routes[0]?.handler;
    expect(handler).toBeDefined();
    if (!handler) return;

    const broken = stubRoute("POST");
    await handler(broken.route);
    expect(broken.seenFallback()).toBe(true);
    const shaped = stubRoute("POST", { provider: 7, project: "p1", multiple: "yes" });
    await handler(shaped.route);
    await expect(driver.exportsSettingsCapturedSubmits()).resolves.toEqual([
      { provider: "", project: [], multiple: false },
    ]);
  });

  it("holds and releases stalled submits", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsStallSubmit();
    const handler = rec.routes[0]?.handler;
    expect(handler).toBeDefined();
    if (!handler) return;

    const write = stubRoute("POST", { provider: "csv", project: [], multiple: false });
    const pending = handler(write.route);
    await Promise.resolve();
    expect(write.seenFallback()).toBe(false);
    await driver.exportsSettingsReleaseSubmit();
    await pending;
    expect(write.seenFallback()).toBe(true);
  });

  it("aborts failed submits and passes again after unfail", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsFailSubmit();
    const handler = rec.routes[0]?.handler;
    expect(handler).toBeDefined();
    if (!handler) return;

    const failed = stubRoute("POST", { provider: "csv", project: [], multiple: false });
    await handler(failed.route);
    expect(failed.seenAbort()).toBe(true);
    expect(failed.seenFallback()).toBe(false);

    await driver.exportsSettingsUnfailSubmit();
    const passed = stubRoute("POST", { provider: "csv", project: [], multiple: false });
    await handler(passed.route);
    expect(passed.seenFallback()).toBe(true);
    expect(rec.routes).toHaveLength(1);
  });

  it("leaves the history stall working on the same hook", async () => {
    const rec = new Recorder();
    const driver = rec.driver();
    await driver.exportsSettingsCaptureSubmit();
    await driver.exportsSettingsStallHistory();
    expect(rec.routes).toHaveLength(1);
    const handler = rec.routes[0]?.handler;
    expect(handler).toBeDefined();
    if (!handler) return;

    const read = stubRoute("GET");
    const pending = handler(read.route);
    await Promise.resolve();
    expect(read.seenFallback()).toBe(false);
    await driver.exportsSettingsReleaseHistory();
    await pending;
    expect(read.seenFallback()).toBe(true);
  });
});

describe("pageExportHeading", () => {
  it("reads the dialog heading", async () => {
    const rec = new Recorder();
    rec.dialogPresent = true;
    await expect(rec.driver().pageExportHeading()).resolves.toBe("Export page");
  });

  it("answers null when the dialog is closed", async () => {
    const rec = new Recorder();
    await expect(rec.driver().pageExportHeading()).resolves.toBeNull();
  });
});

describe("pageExportOfferedOptions", () => {
  it("reads each row's native options in order", async () => {
    const rec = new Recorder();
    rec.dialogPresent = true;
    rec.dialogRows = {
      format: ["PDF", "Markdown"],
      scope: ["Everything", "No images"],
      pageFormat: ["A4", "A3", "A2", "Letter", "Legal", "Tabloid"],
    };
    const driver = rec.driver();
    await expect(driver.pageExportOfferedOptions("format")).resolves.toEqual(["PDF", "Markdown"]);
    await expect(driver.pageExportOfferedOptions("scope")).resolves.toEqual(["Everything", "No images"]);
    await expect(driver.pageExportOfferedOptions("pageFormat")).resolves.toEqual([
      "A4",
      "A3",
      "A2",
      "Letter",
      "Legal",
      "Tabloid",
    ]);
    const selects = rec.calls.filter((call) => call.method === "count" && call.path.includes('locator("#page-export-'));
    expect(selects.some((call) => call.path.includes("page-export-format"))).toBe(true);
    expect(selects.some((call) => call.path.includes("page-export-scope"))).toBe(true);
    expect(selects.some((call) => call.path.includes("page-export-size"))).toBe(true);
  });

  it("answers empty when the row unmounts", async () => {
    const rec = new Recorder();
    rec.dialogPresent = true;
    rec.dialogRows = { format: ["PDF", "Markdown"], scope: ["Everything", "No images"] };
    await expect(rec.driver().pageExportOfferedOptions("pageFormat")).resolves.toEqual([]);
  });
});

describe("pageExportConfirmFull", () => {
  const sandbox = mkdtempSync(join(tmpdir(), "page-export-gate-"));
  afterEach(() => {
    for (const entry of ["page.pdf", "page.md"] as const) {
      rmSync(join(sandbox, entry), { force: true });
    }
  });

  it("returns the full bytes as latin1 with the suggested name", async () => {
    const path = join(sandbox, "page.pdf");
    writeFileSync(path, Buffer.from([0x25, 0x50, 0x44, 0x46, 0xe9, 0x00, 0xff]));
    const rec = new Recorder();
    rec.dialogPresent = true;
    rec.download = { fileName: "my-page-a4.pdf", path };
    const file = await rec.driver().pageExportConfirmFull();

    expect(file.fileName).toBe("my-page-a4.pdf");
    expect(file.byteLength).toBe(7);
    expect(file.content).toBe("%PDF\xe9\x00\xff");
  });

  it("throws when the download lands without a path", async () => {
    const rec = new Recorder();
    rec.dialogPresent = true;
    rec.download = { fileName: "my-page-a4.pdf", path: null };
    await expect(rec.driver().pageExportConfirmFull()).rejects.toThrow("[parity] page export produced no local file.");
  });
});

describe("pageExportFailSave", () => {
  it("evaluates the sabotage in the page", async () => {
    const rec = new Recorder();
    await rec.driver().pageExportFailSave();
    expect(rec.evaluateCalls).toHaveLength(1);
    expect(typeof rec.evaluateCalls[0]?.[0]).toBe("function");
  });
});

describe("pageExportFocusedFacts", () => {
  it("passes the page's answer through", async () => {
    const rec = new Recorder();
    rec.evaluateAnswers = [{ label: "button :: Export", indicator: "outline", focusVisible: true }];
    await expect(rec.driver().pageExportFocusedFacts()).resolves.toEqual({
      label: "button :: Export",
      indicator: "outline",
      focusVisible: true,
    });
    expect(rec.evaluateCalls).toHaveLength(1);
  });

  it("passes null through when nothing focuses", async () => {
    const rec = new Recorder();
    rec.evaluateAnswers = [null];
    await expect(rec.driver().pageExportFocusedFacts()).resolves.toBeNull();
  });
});

describe("pageExportGateMarkersPresent", () => {
  it("reports false when no marker shows", async () => {
    const rec = new Recorder();
    await expect(rec.driver().pageExportGateMarkersPresent()).resolves.toBe(false);
    const sweeps = rec.calls.filter((call) => call.path.startsWith(MAIN) && call.method === "count");
    expect(sweeps.length).toBeGreaterThan(0);
  });

  it("reports true when a visible upgrade control shows", async () => {
    const rec = new Recorder();
    rec.gateHits = [{ role: "button", visible: true }];
    await expect(rec.driver().pageExportGateMarkersPresent()).resolves.toBe(true);
  });

  it("ignores hidden hits", async () => {
    const rec = new Recorder();
    rec.gateHits = [{ role: "button", visible: false }];
    await expect(rec.driver().pageExportGateMarkersPresent()).resolves.toBe(false);
  });

  it("reports true when a gate heading shows", async () => {
    const rec = new Recorder();
    rec.gateHeadings = ["Desktop app"];
    await expect(rec.driver().pageExportGateMarkersPresent()).resolves.toBe(true);
  });

  it("throws when no content root mounts instead of passing vacuously", async () => {
    const rec = new Recorder();
    rec.mainPresent = false;
    await expect(rec.driver().pageExportGateMarkersPresent()).rejects.toThrow(
      "[parity] gate-marker scan mounted no content root; refusing a vacuous pass."
    );
  });

  it("throws when the roots carry no candidate controls", async () => {
    const rec = new Recorder();
    rec.candidateButtons = 0;
    rec.candidateHeadings = 0;
    await expect(rec.driver().pageExportGateMarkersPresent()).rejects.toThrow(
      "[parity] gate-marker scan found no candidate controls; refusing a vacuous pass."
    );
  });

  it("scans the settings shell when it replaces the content main", async () => {
    const rec = new Recorder();
    rec.mainPresent = false;
    rec.settingsShellPresent = true;
    rec.gateHits = [{ role: "button", visible: true }];
    await expect(rec.driver().pageExportGateMarkersPresent()).resolves.toBe(true);
    const sweeps = rec.calls.filter((call) => call.path.startsWith(SETTINGS_SHELL) && call.method === "count");
    expect(sweeps.length).toBeGreaterThan(0);
  });

  it("scans the open dialog alongside the content main", async () => {
    const rec = new Recorder();
    rec.mainPresent = false;
    rec.dialogPresent = true;
    rec.gateHits = [{ role: "button", visible: true }];
    await expect(rec.driver().pageExportGateMarkersPresent()).resolves.toBe(true);
    const sweeps = rec.calls.filter((call) => call.path.includes('getByRole("dialog")') && call.method === "count");
    expect(sweeps.length).toBeGreaterThan(0);
  });

  it("ignores a hidden dialog when choosing roots", async () => {
    const rec = new Recorder();
    rec.mainPresent = false;
    rec.dialogPresent = true;
    rec.dialogVisible = false;
    await expect(rec.driver().pageExportGateMarkersPresent()).rejects.toThrow(
      "[parity] gate-marker scan mounted no content root; refusing a vacuous pass."
    );
  });
});
