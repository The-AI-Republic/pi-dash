// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit coverage for the WebNewDriver typography picks (NEWFRONT-851,
// PAGE-028). A popup that closes hands focus back to the body a beat after
// the pick; a keystroke sent into the closing popup never reaches the
// caret, so the pick waits for the hand-back and says so when it never
// comes.
import type { Locator, Page } from "@playwright/test";
import { describe, expect, it, vi } from "vitest";

import { WebNewDriver } from "./web-new";

function trigger(): Locator {
  return { click: vi.fn(async () => undefined) } as unknown as Locator;
}

function item(): Locator {
  return { click: vi.fn(async () => undefined) } as unknown as Locator;
}

function driverWith(waitForFunction: ReturnType<typeof vi.fn>): {
  driver: WebNewDriver;
  triggerClick: ReturnType<typeof vi.fn>;
  itemClick: ReturnType<typeof vi.fn>;
} {
  const typography = trigger();
  const entry = item();
  const page = {
    getByTestId: vi.fn(() => typography),
    getByRole: vi.fn(() => ({ first: () => entry })),
    waitForFunction,
  } as unknown as Page;
  return {
    driver: new WebNewDriver(page),
    triggerClick: typography.click as unknown as ReturnType<typeof vi.fn>,
    itemClick: entry.click as unknown as ReturnType<typeof vi.fn>,
  };
}

describe("typography pick", () => {
  it("clicks the selector and the named entry, then waits for the body to take the keyboard", async () => {
    const waitForFunction = vi.fn(async () => null);
    const { driver, triggerClick, itemClick } = driverWith(waitForFunction);

    await driver.pagesToolbarTypographyPick("Heading 2");

    expect(triggerClick).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(itemClick).toHaveBeenCalledWith({ timeout: 30_000 });
    expect(waitForFunction).toHaveBeenCalledTimes(1);
    const [probe, arg, options] = waitForFunction.mock.calls[0] as [() => boolean, null, { timeout: number }];
    expect(arg).toBeNull();
    expect(options).toEqual({ timeout: 30_000 });
    // The probe asks whether a key would reach the document: the editor
    // must own the keyboard and have taken it (its own focus binding ran),
    // not merely have been focused by a click.
    const container = document.createElement("div");
    container.className = "editor-container";
    const editable = document.createElement("div");
    editable.className = "ProseMirror";
    editable.tabIndex = 0;
    container.append(editable);
    document.body.append(container);
    editable.focus();
    expect(document.activeElement).toBe(editable);
    expect(probe()).toBe(false);
    editable.classList.add("ProseMirror-focused");
    expect(probe()).toBe(true);
    // A popup owns the keyboard outside the body; the probe says so.
    const popup = document.createElement("div");
    popup.setAttribute("role", "menu");
    const entry = document.createElement("button");
    popup.append(entry);
    document.body.append(popup);
    entry.focus();
    expect(probe()).toBe(false);
    editable.classList.remove("ProseMirror-focused");
    container.remove();
    popup.remove();
  });

  it("fails loudly when the body never takes the keyboard back", async () => {
    const waitForFunction = vi.fn(async () => {
      throw new Error("Timeout 30000ms exceeded.");
    });
    const { driver } = driverWith(waitForFunction);

    await expect(driver.pagesToolbarTypographyPick("Heading 2")).rejects.toThrow(
      "[parity] the body never took the keyboard back after a typography pick."
    );
  });
});
