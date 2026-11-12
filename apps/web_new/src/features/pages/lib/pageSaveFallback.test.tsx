// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Socket-loss fallback persistence (parity/specs/pages.md §8.1, PAGE-064):
// one save fetches the latest binary, reconciles the local body through
// the document convert, and patches all three forms; the hook fires on
// activation, on the idle interval, and early on the debounced manual
// save, and a refused patch reports itself and retries on the next tick.

import { act, renderHook } from "@testing-library/react";
import { ApiError } from "@pidash/api-client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({
  getPageDescriptionBinary: vi.fn(),
  convertDocument: vi.fn(),
  updatePageDescription: vi.fn(),
}));

const shell = vi.hoisted(() => ({
  notifyError: vi.fn(),
}));

vi.mock("@pidash/api-client", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@pidash/api-client")>()),
  getPageDescriptionBinary: contracts.getPageDescriptionBinary,
  convertDocument: contracts.convertDocument,
  updatePageDescription: contracts.updatePageDescription,
}));

vi.mock("../../../shared/shell/toaster.js", () => ({
  notifyError: shell.notifyError,
}));

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { resetPageRealtimeErrors, usePageOversizedIncident } from "./pageRealtimeErrors.js";
import {
  pageBinaryToBase64,
  PAGE_BODY_SAVE_FAILED_TITLE,
  PAGE_SAVE_EMPTY_HTML,
  PAGE_SAVE_FALLBACK_INTERVAL_MS,
  PAGE_SAVE_MANUAL_DEBOUNCE_MS,
  savePageDescriptionFallback,
  usePageSaveFallback,
  type UsePageSaveFallbackOptions,
} from "./pageSaveFallback.js";

const CONVERTED = {
  description_binary: "e30=",
  description_html: "<p>typed</p>",
  description_json: { type: "doc" },
};

function options(overrides: Partial<UsePageSaveFallbackOptions> = {}): UsePageSaveFallbackOptions {
  return {
    workspaceSlug: "acme",
    projectId: "project-1",
    pageId: "page-1",
    title: "Launch notes",
    active: true,
    readHtml: () => "<p>typed</p>",
    ...overrides,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  createAppClient("https://api.test", () => new Promise<never>(() => undefined));
  vi.clearAllMocks();
  contracts.getPageDescriptionBinary.mockResolvedValue(new Uint8Array([104, 105]));
  contracts.convertDocument.mockResolvedValue(CONVERTED);
  contracts.updatePageDescription.mockResolvedValue({ message: "ok" });
});

afterEach(() => {
  vi.useRealTimers();
  resetAppClient();
  resetPageRealtimeErrors();
});

/** Let the hook's queued saves land before asserting. */
async function settle(): Promise<void> {
  await act(async () => {});
}

/**
 * Await the nth patch's promise so its save fully resolves (releasing the
 * in-flight guard) before timers move; tolerant of refused patches.
 */
async function awaitPatch(index: number): Promise<void> {
  const pending = contracts.updatePageDescription.mock.results[index]?.value as Promise<unknown> | undefined;
  await act(async () => {
    await pending?.then(
      () => undefined,
      () => undefined
    );
  });
}

describe("pageBinaryToBase64", () => {
  it("encodes bytes and the empty body", () => {
    expect(pageBinaryToBase64(new Uint8Array([104, 105]))).toBe("aGk=");
    expect(pageBinaryToBase64(new Uint8Array(0))).toBe("");
  });
});

describe("savePageDescriptionFallback", () => {
  it("fetches, reconciles through convert, and patches all three forms", async () => {
    const client = {};
    const converted = await savePageDescriptionFallback(client as never, {
      workspaceSlug: "acme",
      projectId: "project-1",
      pageId: "page-1",
      html: "<p>typed</p>",
      title: "Launch notes",
    });
    expect(converted).toBe(CONVERTED);
    expect(contracts.getPageDescriptionBinary).toHaveBeenCalledWith(client, "acme", "project-1", "page-1");
    expect(contracts.convertDocument).toHaveBeenCalledWith(client, {
      description_html: "<p>typed</p>",
      variant: "document",
      description_binary: "aGk=",
      title: "Launch notes",
    });
    expect(contracts.updatePageDescription).toHaveBeenCalledWith(client, "acme", "project-1", "page-1", {
      description_binary: "e30=",
      description_html: "<p>typed</p>",
      description_json: { type: "doc" },
    });
  });

  it("converts fresh when the server holds no binary yet", async () => {
    contracts.getPageDescriptionBinary.mockResolvedValue(new Uint8Array(0));
    await savePageDescriptionFallback({} as never, {
      workspaceSlug: "acme",
      projectId: "project-1",
      pageId: "page-1",
      html: "<p>typed</p>",
      title: "Launch notes",
    });
    expect(contracts.convertDocument).toHaveBeenCalledWith(
      {},
      {
        description_html: "<p>typed</p>",
        variant: "document",
        title: "Launch notes",
      }
    );
  });

  it("saves the empty document instead of a blank body the convert rejects", async () => {
    await savePageDescriptionFallback({} as never, {
      workspaceSlug: "acme",
      projectId: "project-1",
      pageId: "page-1",
      html: "   ",
      title: "Launch notes",
    });
    expect(contracts.convertDocument).toHaveBeenCalledWith(
      {},
      expect.objectContaining({ description_html: PAGE_SAVE_EMPTY_HTML })
    );
  });
});

describe("usePageSaveFallback", () => {
  it("stays quiet while inactive, even on the manual gesture", async () => {
    const { result } = renderHook(() => usePageSaveFallback(options({ active: false })));
    act(() => {
      result.current.requestManualSave();
    });
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_MANUAL_DEBOUNCE_MS + PAGE_SAVE_FALLBACK_INTERVAL_MS * 2);
    });
    await settle();
    expect(contracts.updatePageDescription).not.toHaveBeenCalled();
  });

  it("patches on activation and then on the idle interval", async () => {
    renderHook(() => usePageSaveFallback(options()));
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(1);
    await awaitPatch(0);
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_FALLBACK_INTERVAL_MS);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(2);
  });

  it("drops a pending manual save on unmount", async () => {
    const { result, unmount } = renderHook(() => usePageSaveFallback(options()));
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(1);
    await awaitPatch(0);
    act(() => {
      result.current.requestManualSave();
    });
    unmount();
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_MANUAL_DEBOUNCE_MS + PAGE_SAVE_FALLBACK_INTERVAL_MS);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(1);
  });

  it("stops the interval once unmounted or deactivated", async () => {
    const hook = renderHook(({ active }) => usePageSaveFallback(options({ active })), {
      initialProps: { active: true },
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(1);
    hook.rerender({ active: false });
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_FALLBACK_INTERVAL_MS * 2);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(1);
    hook.unmount();
  });

  it("debounces the manual save and resets the interval from it", async () => {
    const { result } = renderHook(() => usePageSaveFallback(options()));
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(1);
    await awaitPatch(0);
    act(() => {
      result.current.requestManualSave();
      result.current.requestManualSave();
    });
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_MANUAL_DEBOUNCE_MS);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(2);
    await awaitPatch(1);
    // The interval restarts at the forced patch: nothing fires at the old
    // tick, and the next patch lands a full interval after the gesture.
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_FALLBACK_INTERVAL_MS - 1);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(2);
    await act(async () => {
      vi.advanceTimersByTime(1);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(3);
  });

  it("skips a tick while a patch is still in flight", async () => {
    let release!: (value: { message: string }) => void;
    contracts.updatePageDescription.mockImplementationOnce(
      () => new Promise<{ message: string }>((resolve) => void (release = resolve))
    );
    renderHook(() => usePageSaveFallback(options()));
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(1);
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_FALLBACK_INTERVAL_MS);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(1);
    await act(async () => {
      release({ message: "ok" });
    });
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_FALLBACK_INTERVAL_MS);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(2);
  });

  it("notices a refused patch and retries on the next tick", async () => {
    contracts.updatePageDescription.mockRejectedValueOnce(new Error("boom"));
    renderHook(() => usePageSaveFallback(options()));
    await settle();
    expect(shell.notifyError).toHaveBeenCalledWith(PAGE_BODY_SAVE_FAILED_TITLE);
    await awaitPatch(0);
    await act(async () => {
      vi.advanceTimersByTime(PAGE_SAVE_FALLBACK_INTERVAL_MS);
    });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(2);
  });

  it("raises the oversized strip instead of a notice on a 413", async () => {
    contracts.updatePageDescription.mockRejectedValueOnce(
      new ApiError({ status: 413, code: "http", message: "too large" })
    );
    renderHook(() => usePageSaveFallback(options()));
    const strip = renderHook(() => usePageOversizedIncident("page-1"));
    await settle();
    expect(strip.result.current.visible).toBe(true);
    expect(shell.notifyError).not.toHaveBeenCalled();
  });

  it("restarts the loop when the address moves to another page", async () => {
    const hook = renderHook(
      ({ pageId }) => usePageSaveFallback(options({ pageId, readHtml: () => `<p>${pageId}</p>` })),
      { initialProps: { pageId: "page-1" } }
    );
    await settle();
    await awaitPatch(0);
    hook.rerender({ pageId: "page-2" });
    await settle();
    expect(contracts.updatePageDescription).toHaveBeenCalledTimes(2);
    expect(contracts.convertDocument).toHaveBeenNthCalledWith(
      1,
      expect.anything(),
      expect.objectContaining({ description_html: "<p>page-1</p>" })
    );
    expect(contracts.convertDocument).toHaveBeenNthCalledWith(
      2,
      expect.anything(),
      expect.objectContaining({ description_html: "<p>page-2</p>" })
    );
    hook.unmount();
  });
});
