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
      body: "The sign-in window could not open. Allow pop-ups for this site and try again.",
    });
  });

  it("gives a window closed before authorization its own notice", () => {
    expect(startFailureNotice({ kind: "closed" })).toEqual({
      title: "Sign-in window closed",
      body: "The sign-in window closed before OpenAI answered. Start it again.",
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
    expect(finishNotice({ kind: "confirmed" }).title).toBe("OpenAI account ready");
  });

  it("distinguishes a timeout from a provider-side failure", () => {
    expect(finishNotice({ kind: "timeout" })).toMatchObject({ title: "Sign-in timed out" });
    expect(finishNotice({ kind: "failed", detail: undefined })).toEqual({
      title: "OpenAI sign-in failed",
      body: "OpenAI reported that the sign-in could not finish. Start it again.",
    });
  });

  it("keeps the provider's own detail when it sent one", () => {
    expect(finishNotice({ kind: "failed", detail: "The provider declined" })).toEqual({
      title: "OpenAI sign-in failed",
      body: "The provider declined",
    });
  });
});
