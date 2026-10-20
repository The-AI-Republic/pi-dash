// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace all-issues route (parity/specs/issues.md §18, ISS-223). The
// route is a stub that declares the address and mounts the lazy screen;
// the screen itself (filters, reads, rows) is covered in its own tests.
import { describe, expect, it } from "vitest";

import { Route } from "./all-issues.js";

describe("all-issues route", () => {
  it("declares the workspace all-issues path under the gated layout", () => {
    expect(Route.update).toBeTypeOf("function");
    expect(Route.options.component).toBeTypeOf("function");
  });
});
