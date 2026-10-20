// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@pidash/kit", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  createKitToastManager: () => {
    const manager = {
      add: vi.fn(() => "toast-1"),
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

  it("keeps a pending notice open and settles it in place", () => {
    const pending = notifyPending("Updating", "One moment");
    expect(getToastManager().add).toHaveBeenCalledWith({ title: "Updating", description: "One moment", timeout: 0 });

    pending.settle({ title: "Updated" });
    expect(getToastManager().update).toHaveBeenCalledWith("toast-1", { title: "Updated" });

    pending.settle({ title: "Failed", description: "Try again", failed: true });
    expect(getToastManager().update).toHaveBeenLastCalledWith("toast-1", {
      title: "Failed",
      description: "Try again",
      timeout: 8000,
    });
  });

  it("settles a pending failure with a timed notice", () => {
    notifyPending("Updating").settle({ title: "Failed", failed: true });
    expect(getToastManager().update).toHaveBeenCalledWith("toast-1", { title: "Failed", timeout: 8000 });
  });
});
