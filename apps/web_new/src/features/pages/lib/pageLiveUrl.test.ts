// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Collaboration socket address (parity/specs/pages.md §8.1, §11.6,
// PAGE-065, PAGE-068): the app origin with a socket scheme, the live
// mount path, and the document context query.

import { describe, expect, it } from "vitest";

import { PAGE_LIVE_DOCUMENT_TYPE, PAGE_LIVE_PATH, pageLiveSocketUrl } from "./pageLiveUrl.js";

const MOUNT = { workspaceSlug: "acme", projectId: "project-1" };

describe("pageLiveSocketUrl", () => {
  it("pins the mount path and document kind", () => {
    expect(PAGE_LIVE_PATH).toBe("/live/collaboration");
    expect(PAGE_LIVE_DOCUMENT_TYPE).toBe("project_page");
  });

  it("dials https origins over wss", () => {
    expect(pageLiveSocketUrl("https://app.test", MOUNT)).toBe(
      "wss://app.test/live/collaboration?documentType=project_page&projectId=project-1&workspaceSlug=acme"
    );
  });

  it("dials http origins over ws and keeps the port", () => {
    expect(pageLiveSocketUrl("http://localhost:8000", MOUNT)).toBe(
      "ws://localhost:8000/live/collaboration?documentType=project_page&projectId=project-1&workspaceSlug=acme"
    );
  });

  it("drops the origin's own path, query and hash", () => {
    expect(pageLiveSocketUrl("https://app.test/acme/projects/1/issues?x=1#top", MOUNT)).toBe(
      "wss://app.test/live/collaboration?documentType=project_page&projectId=project-1&workspaceSlug=acme"
    );
  });

  it("encodes context values", () => {
    const url = pageLiveSocketUrl("https://app.test", { workspaceSlug: "ac me", projectId: "a/b" });
    expect(url).toContain("projectId=a%2Fb");
    expect(url).toContain("workspaceSlug=ac+me");
  });

  it("throws on an origin that is not a URL", () => {
    expect(() => pageLiveSocketUrl("not a url", MOUNT)).toThrow();
  });
});
