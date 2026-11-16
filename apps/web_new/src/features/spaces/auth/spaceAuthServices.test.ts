// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { ApiError, createClient, type Transport, type TransportResponse } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import { createDefaultSpaceAuthServices, createSpaceAuthServices, spaceAuthFailureCode } from "./spaceAuthServices.js";

function stubTransport(handler: (url: string) => { status: number; body: string }): Transport {
  return (async (url: string): Promise<TransportResponse> => {
    const response = handler(url);
    return {
      status: response.status,
      url,
      headers: { get: () => null },
      text: async () => response.body,
    };
  }) as Transport;
}

describe("createSpaceAuthServices", () => {
  it("checks addresses through the spaces endpoint", async () => {
    const seen: string[] = [];
    const services = createSpaceAuthServices(
      createClient({
        baseUrl: "https://api.test",
        transport: stubTransport((url) => {
          seen.push(url);
          if (url.endsWith("/auth/get-csrf-token/")) {
            return { status: 200, body: JSON.stringify({ csrf_token: "tok" }) };
          }
          return { status: 200, body: JSON.stringify({ existing: true, status: "MAGIC_CODE" }) };
        }),
      })
    );
    await expect(services.checkEmail("ada@example.com")).resolves.toEqual({
      existing: true,
      status: "MAGIC_CODE",
    });
    expect(seen).toContain("https://api.test/auth/spaces/email-check/");
  });

  it("generates codes and propagates the server refusal code", async () => {
    const ok = createSpaceAuthServices(
      createClient({
        baseUrl: "https://api.test",
        transport: stubTransport(() => ({ status: 200, body: JSON.stringify({ key: "k" }) })),
      })
    );
    await expect(ok.generateCode("ada@example.com")).resolves.toBeUndefined();

    const capped = createSpaceAuthServices(
      createClient({
        baseUrl: "https://api.test",
        transport: stubTransport(() => ({
          status: 400,
          body: JSON.stringify({ error_code: 5102, error_message: "EMAIL_CODE_ATTEMPT_EXHAUSTED_SIGN_UP" }),
        })),
      })
    );
    const failure = await capped.generateCode("ada@example.com").catch((error: unknown) => error);
    expect(failure).toBeInstanceOf(ApiError);
    expect((failure as ApiError).code).toBe("5102");
  });

  it("reads the CSRF token", async () => {
    const services = createSpaceAuthServices(
      createClient({
        baseUrl: "https://api.test",
        transport: stubTransport(() => ({ status: 200, body: JSON.stringify({ csrf_token: "tok" }) })),
      })
    );
    await expect(services.readCsrfToken()).resolves.toBe("tok");
  });
});

describe("spaceAuthFailureCode", () => {
  it("passes server numbers through and labels foreign failures", () => {
    expect(spaceAuthFailureCode(new ApiError({ code: "5102", message: "capped" }))).toBe("5102");
    expect(spaceAuthFailureCode(new ApiError({ code: "network", message: "down" }))).toBe("network");
    expect(spaceAuthFailureCode(new Error("boom"))).toBe("network");
    expect(spaceAuthFailureCode(null)).toBe("network");
  });
});

describe("createDefaultSpaceAuthServices", () => {
  it("builds services without a window", () => {
    expect(typeof globalThis.window).toBe("undefined");
    const services = createDefaultSpaceAuthServices();
    expect(typeof services.checkEmail).toBe("function");
    expect(typeof services.generateCode).toBe("function");
    expect(typeof services.readCsrfToken).toBe("function");
  });
});
