// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Unit tests for the endpoint-traffic classifier (NEWFRONT-772, EXP-024).
import { describe, expect, it } from "vitest";

import { isParityApiRequestPath } from "./api-traffic.js";

describe("isParityApiRequestPath", () => {
  it("accepts endpoint calls under /api/", () => {
    expect(isParityApiRequestPath("http://localhost:3072/api/workspaces/acme/export-issues")).toBe(true);
    expect(isParityApiRequestPath("http://localhost:3072/api/health/?verbose=1")).toBe(true);
  });

  it("rejects dev-server module fetches mentioning api", () => {
    expect(isParityApiRequestPath("http://localhost:3072/src/features/cloud-billing/api/keys.ts")).toBe(false);
    expect(isParityApiRequestPath("http://localhost:3072/src/core/api/client.ts?t=123")).toBe(false);
    expect(isParityApiRequestPath("http://localhost:3072/@fs/home/app/src/api/x.ts")).toBe(false);
  });

  it("rejects pages, sockets and unparseable URLs", () => {
    expect(isParityApiRequestPath("http://localhost:3072/acme/settings/exports")).toBe(false);
    expect(isParityApiRequestPath("ws://localhost:3072/live/collaboration")).toBe(false);
    expect(isParityApiRequestPath("not a url")).toBe(false);
    expect(isParityApiRequestPath("")).toBe(false);
  });
});
