// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { afterEach, describe, expect, it } from "vitest";

import { clearWorkItemDraft, loadWorkItemDraft, storeWorkItemDraft, workItemDraftKey } from "./createDraft.js";

afterEach(() => {
  globalThis.localStorage.clear();
});

describe("work item draft", () => {
  it("namespaces the key per workspace", () => {
    expect(workItemDraftKey("acme")).toBe("pidash:work-item-draft:acme");
  });

  it("round-trips the unfinished form", () => {
    expect(loadWorkItemDraft("acme")).toBeNull();
    storeWorkItemDraft("acme", { title: "Half typed", projectId: "p1" });
    expect(loadWorkItemDraft("acme")).toEqual({ title: "Half typed", projectId: "p1" });
    storeWorkItemDraft("acme", { title: "No home", projectId: null });
    expect(loadWorkItemDraft("acme")).toEqual({ title: "No home", projectId: null });
    expect(loadWorkItemDraft("globex")).toBeNull();
    clearWorkItemDraft("acme");
    expect(loadWorkItemDraft("acme")).toBeNull();
  });

  it("reads corrupt or foreign payloads as no draft", () => {
    globalThis.localStorage.setItem("pidash:work-item-draft:acme", "{oops");
    expect(loadWorkItemDraft("acme")).toBeNull();
    globalThis.localStorage.setItem("pidash:work-item-draft:acme", '"just a string"');
    expect(loadWorkItemDraft("acme")).toBeNull();
    globalThis.localStorage.setItem("pidash:work-item-draft:acme", "null");
    expect(loadWorkItemDraft("acme")).toBeNull();
    globalThis.localStorage.setItem("pidash:work-item-draft:acme", JSON.stringify({ title: 7, projectId: "p1" }));
    expect(loadWorkItemDraft("acme")).toBeNull();
    globalThis.localStorage.setItem("pidash:work-item-draft:acme", JSON.stringify({ title: "ok", projectId: 7 }));
    expect(loadWorkItemDraft("acme")).toBeNull();
  });
});
