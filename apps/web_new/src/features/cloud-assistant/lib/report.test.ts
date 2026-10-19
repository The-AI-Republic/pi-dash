// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-in outcomes (CLOUD-033): each way the flow can end reports its own
// title and body, and a provider detail wins over the generic failure copy.
import { describe, expect, it } from "vitest";

import { ApiError } from "@pidash/api-client";

import { finishNotice, startFailureNotice } from "./report.js";

describe("startFailureNotice", () => {
  it("tells a blocked popup how to allow it", () => {
    expect(startFailureNotice({ kind: "blocked" })).toEqual({
      title: "Popup blocked",
      body: "Allow popups for this site, then try again.",
    });
  });

  it("reports a tab closed before authorization as a failed sign-in", () => {
    expect(startFailureNotice({ kind: "closed" })).toEqual({
      title: "OpenAI sign-in failed",
      body: "The ChatGPT sign-in tab was closed. Try again.",
    });
  });

  it("uses the server's detail for a rejected start", () => {
    expect(
      startFailureNotice({
        kind: "rejected",
        error: new ApiError({ status: 400, code: "codex_unavailable", message: "Codex is off" }),
      })
    ).toEqual({ title: "Could not start ChatGPT sign-in", body: "Codex is off" });
  });

  it("falls back for a rejected start with no detail", () => {
    expect(startFailureNotice({ kind: "rejected", error: new Error("") })).toEqual({
      title: "Could not start ChatGPT sign-in",
      body: "Unknown error",
    });
  });
});

describe("finishNotice", () => {
  it("confirms a completion", () => {
    expect(finishNotice({ kind: "confirmed" }).title).toBe("ChatGPT plan connected");
  });

  it("distinguishes a timeout from a provider-side failure", () => {
    expect(finishNotice({ kind: "timeout" })).toMatchObject({ title: "Sign-in timed out" });
    expect(finishNotice({ kind: "failed", detail: undefined })).toEqual({
      title: "OpenAI sign-in failed",
      body: "The sign-in could not be completed. Try again.",
    });
  });

  it("keeps the provider's own detail when it sent one", () => {
    expect(finishNotice({ kind: "failed", detail: "The provider declined" })).toEqual({
      title: "OpenAI sign-in failed",
      body: "The provider declined",
    });
  });
});
