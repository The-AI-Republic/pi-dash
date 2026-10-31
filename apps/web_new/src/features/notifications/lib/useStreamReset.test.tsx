// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { useStreamReset } from "./useStreamReset.js";

describe("useStreamReset (NTF-015/016)", () => {
  it("does not re-read on the first render", () => {
    const refetch = vi.fn(async () => undefined);
    renderHook(() => useStreamReset("all||false|false|false", refetch));
    expect(refetch).not.toHaveBeenCalled();
  });

  it("re-reads when the stream signature changes", () => {
    const refetch = vi.fn(async () => undefined);
    const { rerender } = renderHook(({ signature }) => useStreamReset(signature, refetch), {
      initialProps: { signature: "all||false|false|false" },
    });
    rerender({ signature: "all|assigned|false|false|false" });
    expect(refetch).toHaveBeenCalledTimes(1);
    rerender({ signature: "all|assigned|false|false|false" });
    expect(refetch).toHaveBeenCalledTimes(1);
    rerender({ signature: "mentions|assigned|false|false|false" });
    expect(refetch).toHaveBeenCalledTimes(2);
  });

  it("calls the latest refetch without depending on its identity", () => {
    const first = vi.fn(async () => undefined);
    const second = vi.fn(async () => undefined);
    const { rerender } = renderHook(({ signature, refetch }) => useStreamReset(signature, refetch), {
      initialProps: { signature: "a", refetch: first },
    });
    rerender({ signature: "a", refetch: second });
    expect(second).not.toHaveBeenCalled();
    rerender({ signature: "b", refetch: second });
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
