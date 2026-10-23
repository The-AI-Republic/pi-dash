// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Today's-activity export (parity/specs/profile.md §3, PROF-007): the
// local date travels to the server and the CSV bytes reach a download
// sink named for the export.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { ApiClient } from "@pidash/api-client";
import { exportUserProfileActivity } from "@pidash/api-client";

import { activityExportFilename, downloadProfileActivity, localIsoDate, type ExportSink } from "./export.js";

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, exportUserProfileActivity: vi.fn() };
});

beforeEach(() => {
  Object.defineProperty(URL, "createObjectURL", { value: () => "blob:stub", configurable: true, writable: true });
  Object.defineProperty(URL, "revokeObjectURL", { value: () => undefined, configurable: true, writable: true });
});

afterEach(() => {
  vi.restoreAllMocks();
});

function sink(): ExportSink & { saved: Array<[string, string]>; revoked: string[]; blob: Blob | null } {
  const saved: Array<[string, string]> = [];
  const revoked: string[] = [];
  const state = {
    saved,
    revoked,
    blob: null as Blob | null,
    createObjectUrl: (blob: Blob) => {
      state.blob = blob;
      return "blob:profile";
    },
    revokeObjectUrl: (url: string) => {
      revoked.push(url);
    },
    save: (url: string, filename: string) => {
      saved.push([url, filename]);
    },
    now: () => 1_700_000_000_000,
  };
  return state;
}

describe("localIsoDate", () => {
  it("reads the viewer's local calendar day, zero-padded", () => {
    expect(localIsoDate(new Date(2026, 0, 2, 23, 30))).toBe("2026-01-02");
    expect(localIsoDate(new Date(2026, 10, 19, 0, 5))).toBe("2026-11-19");
  });
});

describe("activityExportFilename", () => {
  it("names one file per click", () => {
    expect(activityExportFilename(1700000000000)).toBe("profile-activity-1700000000000.csv");
  });
});

describe("downloadProfileActivity", () => {
  it("posts the local day and saves the CSV under the export name", async () => {
    vi.mocked(exportUserProfileActivity).mockResolvedValue({
      csv: "actor,issue\nOpal,PP-1",
      contentType: "text/csv; charset=utf-8",
      status: 200,
      filename: "activity.csv",
    });
    const target = sink();
    const exported = await downloadProfileActivity({} as ApiClient, "acme", "u-1", target);
    expect(exported.csv).toContain("PP-1");
    expect(exportUserProfileActivity).toHaveBeenCalledWith({} as ApiClient, "acme", "u-1", localIsoDate());
    expect(target.saved).toEqual([["blob:profile", "profile-activity-1700000000000.csv"]]);
    expect(target.blob?.type).toBe("text/csv; charset=utf-8");
    expect(target.revoked).toEqual(["blob:profile"]);
  });

  it("stamps the click time before the export resolves and ignores the server name", async () => {
    let release!: (value: { csv: string; contentType: string; status: number; filename: string }) => void;
    vi.mocked(exportUserProfileActivity).mockReturnValue(
      new Promise((resolve) => {
        release = resolve;
      })
    );
    const target = sink();
    let nowCalls = 0;
    const realNow = target.now;
    target.now = () => {
      nowCalls += 1;
      return realNow();
    };
    const pending = downloadProfileActivity({} as ApiClient, "acme", "u-1", target);
    expect(nowCalls).toBe(1);
    expect(exportUserProfileActivity).toHaveBeenCalledTimes(1);
    release({ csv: "actor", contentType: "text/csv", status: 200, filename: "workspace-user-activity.csv" });
    const exported = await pending;
    expect(exported.filename).toBe("workspace-user-activity.csv");
    expect(target.saved).toEqual([["blob:profile", "profile-activity-1700000000000.csv"]]);
  });

  it("defaults a blank content type to text/csv and revokes even when saving throws", async () => {
    vi.mocked(exportUserProfileActivity).mockResolvedValue({
      csv: "actor",
      contentType: "",
      status: 200,
      filename: "activity.csv",
    });
    const target = sink();
    target.save = () => {
      throw new Error("blocked");
    };
    await expect(downloadProfileActivity({} as ApiClient, "acme", "u-1", target)).rejects.toThrow("blocked");
    expect(target.blob?.type).toBe("text/csv");
    expect(target.revoked).toEqual(["blob:profile"]);
  });

  it("takes the browser sink by default and clicks an anchor", async () => {
    vi.mocked(exportUserProfileActivity).mockResolvedValue({
      csv: "actor",
      contentType: "text/csv",
      status: 200,
      filename: "activity.csv",
    });
    const create = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:live");
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    await downloadProfileActivity({} as ApiClient, "acme", "u-1");
    expect(create).toHaveBeenCalledTimes(1);
    expect(click).toHaveBeenCalledTimes(1);
    expect(revoke).toHaveBeenCalledWith("blob:live");
    expect(document.querySelector("a[download]")).toBeNull();
  });
});
