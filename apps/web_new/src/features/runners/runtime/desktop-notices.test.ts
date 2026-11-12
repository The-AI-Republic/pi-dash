// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it, vi } from "vitest";

import type { AgentRuntimeApi, NoticesView } from "../../../core/platform/index.js";
import {
  dismissNotices,
  emptyNotices,
  folderKey,
  hasNotices,
  loadNotices,
  podKey,
  recordNotices,
  replaceKey,
  subscribeNotices,
} from "./desktop-notices.js";

function fakeRuntime(view: NoticesView): AgentRuntimeApi & {
  loadNotices: ReturnType<typeof vi.fn>;
  recordNotices: ReturnType<typeof vi.fn>;
  dismissNotices: ReturnType<typeof vi.fn>;
} {
  return {
    loadNotices: vi.fn(async () => view),
    recordNotices: vi.fn(async () => view),
    dismissNotices: vi.fn(async () => view),
  } as unknown as AgentRuntimeApi & {
    loadNotices: ReturnType<typeof vi.fn>;
    recordNotices: ReturnType<typeof vi.fn>;
    dismissNotices: ReturnType<typeof vi.fn>;
  };
}

describe("desktop notices keys and emptiness", () => {
  it("keys notices by path, runner and machine", () => {
    expect(folderKey({ workspace: "acme", project: "WEB", path: "/tasks" })).toBe("folder:/tasks");
    expect(podKey({ runner_id: "r-1", workspace: "acme", project: "WEB" })).toBe("pod:r-1");
    expect(replaceKey("m-1")).toBe("replace:m-1");
  });

  it("is empty when there are no notices", () => {
    expect(hasNotices(emptyNotices())).toBe(false);
    expect(
      hasNotices({ legacy_folders: [{ workspace: "acme", project: "WEB", path: "/tasks" }], pod_member_projects: [] })
    ).toBe(true);
    expect(
      hasNotices({ legacy_folders: [], pod_member_projects: [{ runner_id: "r-1", workspace: "acme", project: "WEB" }] })
    ).toBe(true);
  });
});

describe("load, record and dismiss", () => {
  it("delegates to the host and notifies subscribers", async () => {
    const view: NoticesView = {
      notices: { legacy_folders: [{ workspace: "acme", project: "WEB", path: "/tasks" }], pod_member_projects: [] },
      dismissed: [],
    };
    const runtime = fakeRuntime(view);
    const seen: string[] = [];
    const stop = subscribeNotices(() => seen.push("changed"));
    await expect(loadNotices("u-1", runtime)).resolves.toEqual(view);
    expect(runtime.loadNotices).toHaveBeenCalledWith("u-1");
    const notices = { legacy_folders: [], pod_member_projects: [] };
    await recordNotices("u-1", notices, runtime);
    expect(runtime.recordNotices).toHaveBeenCalledWith("u-1", notices);
    await dismissNotices("u-1", ["folder:/tasks"], runtime);
    expect(runtime.dismissNotices).toHaveBeenCalledWith("u-1", ["folder:/tasks"]);
    // load does not notify; record and dismiss do.
    expect(seen).toEqual(["changed", "changed"]);
    stop();
  });

  it("stops notifying after unsubscribe", async () => {
    const runtime = fakeRuntime({ notices: { legacy_folders: [], pod_member_projects: [] }, dismissed: [] });
    const listener = vi.fn();
    const stop = subscribeNotices(listener);
    stop();
    await recordNotices("u-1", { legacy_folders: [], pod_member_projects: [] }, runtime);
    expect(listener).not.toHaveBeenCalled();
  });
});
