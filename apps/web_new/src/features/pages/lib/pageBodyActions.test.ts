// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copy-body-as-markup entry (parity/specs/pages.md §6.1, PAGE-044). The
// clipboard write precedes the notice; a denied write reports failure,
// and the registry only exists while an editable surface is mounted.

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  copyPageMarkdown,
  getPageBodyActions,
  PAGE_COPY_MARKDOWN_FAILURE,
  PAGE_COPY_MARKDOWN_LABEL,
  PAGE_COPY_MARKDOWN_SUCCESS,
  pageBodyActionsFor,
  registerPageBodyActions,
  resetPageBodyActions,
  subscribePageBodyActions,
} from "./pageBodyActions.js";

const toaster = vi.hoisted(() => ({ notifySuccess: vi.fn(), notifyError: vi.fn() }));
vi.mock("../../../shared/shell/toaster.js", () => toaster);

afterEach(() => {
  resetPageBodyActions();
  toaster.notifySuccess.mockReset();
  toaster.notifyError.mockReset();
  vi.unstubAllGlobals();
});

describe("pageBodyActions registry", () => {
  it("publishes the live actions and clears them on dispose", () => {
    const dispose = registerPageBodyActions(pageBodyActionsFor(() => "<p>x</p>"));
    expect(getPageBodyActions()).not.toBeNull();
    dispose();
    expect(getPageBodyActions()).toBeNull();
  });

  it("a later registration wins and an older disposer leaves it alone", () => {
    const first = registerPageBodyActions(pageBodyActionsFor(() => "<p>one</p>"));
    const second = registerPageBodyActions(pageBodyActionsFor(() => "<p>two</p>"));
    first();
    expect(getPageBodyActions()).not.toBeNull();
    second();
    expect(getPageBodyActions()).toBeNull();
  });

  it("names the entry Copy markdown", () => {
    expect(pageBodyActionsFor(() => "").copyMarkdownLabel).toBe(PAGE_COPY_MARKDOWN_LABEL);
  });

  it("notifies subscribers when the mounted entry appears and disappears", () => {
    const seen: Array<boolean> = [];
    const unsubscribe = subscribePageBodyActions(() => seen.push(getPageBodyActions() !== null));
    const dispose = registerPageBodyActions(pageBodyActionsFor(() => ""));
    dispose();
    unsubscribe();
    registerPageBodyActions(pageBodyActionsFor(() => ""));
    expect(seen).toEqual([true, false]);
  });
});

describe("copyPageMarkdown", () => {
  it("writes Markdown and then notifies success", async () => {
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await copyPageMarkdown(() => "<h1>Title</h1><p>body</p>");
    expect(writeText).toHaveBeenCalledWith("# Title\n\nbody\n");
    expect(toaster.notifySuccess).toHaveBeenCalledWith(PAGE_COPY_MARKDOWN_SUCCESS);
  });

  it("reports a refused clipboard write and never claims success", async () => {
    vi.stubGlobal("navigator", {
      clipboard: {
        writeText: vi.fn(async () => {
          throw new Error("denied");
        }),
      },
    });
    await copyPageMarkdown(() => "<p>body</p>");
    expect(toaster.notifySuccess).not.toHaveBeenCalled();
    expect(toaster.notifyError).toHaveBeenCalledWith(PAGE_COPY_MARKDOWN_FAILURE, "denied");
  });

  it("reports a non-Error rejection without a description", async () => {
    vi.stubGlobal("navigator", {
      clipboard: {
        writeText: vi.fn(async () => {
          throw "no";
        }),
      },
    });
    await copyPageMarkdown(() => "<p>body</p>");
    expect(toaster.notifyError).toHaveBeenCalledWith(PAGE_COPY_MARKDOWN_FAILURE, undefined);
  });
});

describe("pageBodyActionsFor", () => {
  it("copies through the live reader when invoked", async () => {
    const writeText = vi.fn(async () => undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await pageBodyActionsFor(() => "<p>live</p>").copyMarkdown();
    expect(writeText).toHaveBeenCalledWith("live\n");
  });
});
