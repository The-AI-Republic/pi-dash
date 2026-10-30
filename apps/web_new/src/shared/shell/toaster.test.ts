// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/kit", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  createKitToastManager: () => {
    const add: ReturnType<typeof vi.fn> = vi.fn(() => `toast-${add.mock.calls.length}`);
    const manager = {
      add,
      update: vi.fn(),
      close: vi.fn(),
      promise: vi.fn(),
    };
    return manager;
  },
}));

import { getToastManager, notifyError, notifyPending, notifySuccess } from "./toaster.js";

afterEach(() => {
  vi.clearAllMocks();
});

describe("toaster", () => {
  it("adds a plain success with its description", () => {
    notifySuccess("Saved", "All good");
    expect(getToastManager().add).toHaveBeenCalledWith(
      expect.objectContaining({ title: "Saved", description: "All good" })
    );
  });

  it("omits an absent description", () => {
    notifyError("Failed");
    expect(getToastManager().add).toHaveBeenCalledWith(expect.objectContaining({ title: "Failed", timeout: 8000 }));
  });

  it("keeps a pending notice open until it settles", () => {
    const pending = notifyPending("Updating", "One moment");
    expect(getToastManager().add).toHaveBeenCalledWith({ title: "Updating", description: "One moment", timeout: 0 });
    expect(pending.id).toBe("toast-1");
  });

  it("replaces the pending notice with a timed success that auto-dismisses", () => {
    const pending = notifyPending("Updating");
    pending.settle({ title: "Updated", description: "All good" });

    // A sticky toast cannot be retimed in place: the outcome is a new
    // toast and the pending one closes, so a success never stays up.
    expect(getToastManager().close).toHaveBeenCalledWith("toast-1");
    expect(getToastManager().add).toHaveBeenLastCalledWith({
      title: "Updated",
      description: "All good",
      timeout: 5000,
      type: "success",
    });
    expect(getToastManager().update).not.toHaveBeenCalled();
    expect(pending.id).toBe("toast-2");
  });

  it("settles a failure into a longer-lived timed notice", () => {
    const pending = notifyPending("Updating");
    pending.settle({ title: "Failed", description: "Try again", failed: true });
    expect(getToastManager().close).toHaveBeenCalledWith("toast-1");
    expect(getToastManager().add).toHaveBeenLastCalledWith({
      title: "Failed",
      description: "Try again",
      timeout: 8000,
      type: "error",
    });
  });

  it("omits an absent outcome description", () => {
    notifyPending("Updating", "One moment").settle({ title: "Updated" });
    expect(getToastManager().add).toHaveBeenLastCalledWith({ title: "Updated", timeout: 5000, type: "success" });
  });

  it("swaps a second outcome for the first and closes the one it replaced", () => {
    const pending = notifyPending("Updating");
    pending.settle({ title: "Updated" });
    pending.settle({ title: "Failed", failed: true });
    expect(getToastManager().close).toHaveBeenNthCalledWith(1, "toast-1");
    expect(getToastManager().close).toHaveBeenNthCalledWith(2, "toast-2");
    expect(getToastManager().add).toHaveBeenLastCalledWith({ title: "Failed", timeout: 8000, type: "error" });
    expect(pending.id).toBe("toast-3");
  });
});
