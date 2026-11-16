// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Contract: cloud session refresh, cloud CSRF + sign-out, desktop code
// exchange, and the billing plan snapshot against live Django. Skipped
// without PIDASH_CONTRACT_BASE_URL (see the runbook). These paths are
// cloud-only — the private overlay serves them, so the OSS contract
// stack answers 404. These suites pin the wiring (exact method + path,
// recorded at the live boundary) and the clean 404 error surface; the
// cloud positives (rotated cookies, logout answer, exchange landings,
// plan snapshot, 402 refusal) are proven by unit stubs and by the CLOUD
// parity rows at the area gate on the cloud stack.
import { describe, expect, it } from "vitest";
import { createClient } from "../client.js";
import type { Transport } from "../client.js";
import { getCloudBillingPlan } from "../contracts/cloud-billing.js";
import { cloudSignOut, exchangeDesktopCode, refreshCloudSession } from "../contracts/cloud-session.js";
import { ApiError } from "../errors.js";
import { CONTRACT_BASE_URL, contractEnabled, contractTransport } from "./setup.js";

interface RecordedLiveCall {
  method: string;
  url: string;
}

/**
 * The live redirect-walking transport with a request log, so each test
 * proves the exact method + path that reached Django — a bare 404
 * assertion alone could not tell the cloud path from a typo.
 */
function recordingLiveTransport(calls: RecordedLiveCall[]): Transport {
  const live = contractTransport();
  return async (url, init) => {
    calls.push({ method: init.method, url });
    return live(url, init);
  };
}

function liveClient(calls: RecordedLiveCall[]) {
  if (!contractEnabled) throw new Error("PIDASH_CONTRACT_BASE_URL is not set");
  return createClient({ baseUrl: CONTRACT_BASE_URL, transport: recordingLiveTransport(calls), validate: true });
}

function expectRecorded(calls: RecordedLiveCall[], method: string, suffix: string): void {
  expect(calls).toHaveLength(1);
  expect(calls[0]?.method).toBe(method);
  expect(calls[0]?.url.endsWith(suffix)).toBe(true);
}

describe.skipIf(!contractEnabled)("cloud session refresh contract", () => {
  it("posts the rotation to the cloud path and surfaces the OSS 404", async () => {
    // One call, no CSRF prefetch: the refresh authenticates by cookie
    // alone. The cloud stack answers 200 with a rotated pair; the OSS
    // stack has no such path.
    const calls: RecordedLiveCall[] = [];
    const error = await refreshCloudSession(liveClient(calls)).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
    expectRecorded(calls, "POST", "/api/auth/refresh/");
  });
});

describe.skipIf(!contractEnabled)("cloud sign-out contract", () => {
  it("reads the token from the cloud CSRF path, posting nothing when it is absent", async () => {
    // The /api/ prefix discriminates from the OSS twin the default CSRF
    // plumbing reads (/auth/get-csrf-token/, 200 on this stack): a path
    // mixup would read a token and post, not fail here.
    const calls: RecordedLiveCall[] = [];
    const error = await cloudSignOut(liveClient(calls)).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
    expectRecorded(calls, "GET", "/api/auth/get-csrf-token/");
  });
});

describe.skipIf(!contractEnabled)("cloud desktop exchange contract", () => {
  it("trades the deep-link pair through the exchange query", async () => {
    // The cloud stack answers a redirect the parser reads; the OSS
    // stack has no such path. The recorded query proves the pair
    // travels as code/state params.
    const calls: RecordedLiveCall[] = [];
    const client = liveClient(calls);
    const error = await exchangeDesktopCode(client, { code: "probe", state: "probe-state" }).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
    expect(calls).toHaveLength(1);
    expect(calls[0]?.method).toBe("GET");
    const url = new URL(calls[0]?.url ?? "");
    expect(url.pathname).toBe("/api/auth/desktop-exchange/");
    expect(url.searchParams.get("code")).toBe("probe");
    expect(url.searchParams.get("state")).toBe("probe-state");
  });
});

describe.skipIf(!contractEnabled)("cloud billing plan contract", () => {
  it("reads the plan snapshot from the billing path", async () => {
    // The cloud stack answers the snapshot; the OSS stack has no such
    // path. (The 402 quota shape has no live proof here either — it
    // needs a capped cloud account — so unit stubs prove that shape.)
    const calls: RecordedLiveCall[] = [];
    const error = await getCloudBillingPlan(liveClient(calls)).catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(404);
    expectRecorded(calls, "GET", "/api/billing/me/plan/");
  });
});
