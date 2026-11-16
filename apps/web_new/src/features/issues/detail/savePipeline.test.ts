// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { useSavePipeline } from "./savePipeline.js";

function unloadPrevented(): boolean {
  const event = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(event);
  return event.defaultPrevented;
}

describe("useSavePipeline", () => {
  it("starts saved and moves through saving back to saved", () => {
    const { result } = renderHook(() => useSavePipeline());
    expect(result.current.status).toBe("saved");
    act(() => {
      result.current.markSaving();
    });
    expect(result.current.status).toBe("saving");
    act(() => {
      result.current.markSaved();
    });
    expect(result.current.status).toBe("saved");
  });

  it("warns before unload only while a save is in flight", () => {
    const { result, unmount } = renderHook(() => useSavePipeline());
    expect(unloadPrevented()).toBe(false);
    act(() => {
      result.current.markSaving();
    });
    expect(unloadPrevented()).toBe(true);
    act(() => {
      result.current.markSaved();
    });
    expect(unloadPrevented()).toBe(false);
    act(() => {
      result.current.markSaving();
    });
    unmount();
    expect(unloadPrevented()).toBe(false);
  });
});
