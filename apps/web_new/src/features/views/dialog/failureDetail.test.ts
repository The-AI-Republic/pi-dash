// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";

import { failureDetail } from "./failureDetail.js";

describe("failureDetail", () => {
  it("names the server detail when the body carries one", () => {
    expect(failureDetail({ body: { detail: "Name taken" } }, "fallback")).toBe("Name taken");
  });

  it("falls back when the body carries no detail", () => {
    expect(failureDetail({ body: {} }, "fallback")).toBe("fallback");
    expect(failureDetail({ body: { detail: "" } }, "fallback")).toBe("fallback");
    expect(failureDetail({ body: { detail: 42 } }, "fallback")).toBe("fallback");
  });

  it("falls back when there is no body to read", () => {
    expect(failureDetail(new Error("offline"), "fallback")).toBe("fallback");
    expect(failureDetail({ body: null }, "fallback")).toBe("fallback");
    expect(failureDetail({ body: ["detail"] }, "fallback")).toBe("fallback");
    expect(failureDetail({ body: "Detail" }, "fallback")).toBe("fallback");
  });
});
