// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { SpaceAuthServices } from "./spaceAuthServices.js";
import { useSpaceCsrfToken } from "./spaceAuthServices.js";

function servicesWith(readCsrfToken: () => Promise<string>): SpaceAuthServices {
  return {
    checkEmail: () => Promise.reject(new Error("unused")),
    generateCode: () => Promise.reject(new Error("unused")),
    readCsrfToken,
  };
}

describe("useSpaceCsrfToken", () => {
  it("resolves the token", async () => {
    const { result } = renderHook(() => useSpaceCsrfToken(servicesWith(() => Promise.resolve("tok"))));
    await waitFor(() => expect(result.current.token).toBe("tok"));
    expect(result.current.failed).toBe(false);
  });

  it("reports a failed read", async () => {
    const { result } = renderHook(() => useSpaceCsrfToken(servicesWith(() => Promise.reject(new Error("down")))));
    await waitFor(() => expect(result.current.failed).toBe(true));
    expect(result.current.token).toBeNull();
  });

  it("drops late answers after unmount", async () => {
    let resolveRead!: (value: string) => void;
    const readCsrfToken = vi.fn(
      () =>
        new Promise<string>((resolve) => {
          resolveRead = resolve;
        })
    );
    const { result, unmount } = renderHook(() => useSpaceCsrfToken(servicesWith(readCsrfToken)));
    expect(result.current.token).toBeNull();
    unmount();
    await act(async () => {
      resolveRead("late");
    });
    expect(readCsrfToken).toHaveBeenCalledTimes(1);
  });

  it("drops late failures after unmount", async () => {
    let rejectRead!: (error: Error) => void;
    const readCsrfToken = vi.fn(
      () =>
        new Promise<string>((_resolve, reject) => {
          rejectRead = reject;
        })
    );
    const { unmount } = renderHook(() => useSpaceCsrfToken(servicesWith(readCsrfToken)));
    unmount();
    await act(async () => {
      rejectRead(new Error("down"));
    });
    expect(readCsrfToken).toHaveBeenCalledTimes(1);
  });
});
