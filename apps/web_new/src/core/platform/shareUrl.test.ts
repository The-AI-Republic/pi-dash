// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Copied-link origin rules (DESK-019): a shareable path resolves against
// the configured hosted app, keeps its query and fragment, and is refused —
// never quietly pointed at the local bundle origin — when the build has no
// usable hosted address.

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  bakedHostedOrigin,
  desktopShareUrl,
  HOSTED_ORIGIN_REQUIRED,
  HOSTED_ORIGIN_SCHEME,
  pageOrigin,
  parseHostedOrigin,
  resolveShareUrl,
  webShareUrl,
} from "./shareUrl.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("hosted-origin resolution", () => {
  it("keeps the path, query and fragment on the hosted origin", () => {
    expect(desktopShareUrl("/acme/browse/ACME-12/?pane=info#comment-3", "https://app.example.test")).toBe(
      "https://app.example.test/acme/browse/ACME-12/?pane=info#comment-3"
    );
  });

  it("tolerates a trailing slash or surrounding spaces on the base", () => {
    expect(resolveShareUrl("/acme/browse/ACME-1/", "  https://app.example.test///  ")).toBe(
      "https://app.example.test/acme/browse/ACME-1/"
    );
  });

  it("refuses without a configured origin", () => {
    expect(() => resolveShareUrl("/acme", undefined)).toThrow(HOSTED_ORIGIN_REQUIRED);
    expect(() => resolveShareUrl("/acme", "")).toThrow(HOSTED_ORIGIN_REQUIRED);
    expect(() => resolveShareUrl("/acme", "   ")).toThrow(HOSTED_ORIGIN_REQUIRED);
  });

  it("refuses a configured origin that is not a web address", () => {
    expect(() => resolveShareUrl("/acme", "tauri://localhost")).toThrow(HOSTED_ORIGIN_SCHEME);
    expect(() => resolveShareUrl("/acme", "ftp://app.example.test")).toThrow(HOSTED_ORIGIN_SCHEME);
    expect(() => resolveShareUrl("/acme", "not a url")).toThrow(HOSTED_ORIGIN_SCHEME);
  });

  it("reads the hosted origin out of the build define", () => {
    expect(bakedHostedOrigin()).toBeUndefined();
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "https://app.example.test/");
    expect(bakedHostedOrigin()).toBe("https://app.example.test");
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "tauri://localhost");
    expect(bakedHostedOrigin()).toBeUndefined();
  });

  it("resolves desktop links against the baked origin by default", () => {
    vi.stubGlobal("__PIDASH_WEB_BASE_URL__", "https://app.example.test");
    expect(desktopShareUrl("/acme/browse/ACME-1/")).toBe("https://app.example.test/acme/browse/ACME-1/");
  });

  it("keeps the page origin on the web platform", () => {
    expect(webShareUrl("/acme/browse/ACME-1/")).toBe(`${globalThis.location.origin}/acme/browse/ACME-1/`);
  });

  it("parses only absolute http(s) addresses", () => {
    expect(parseHostedOrigin("https://app.example.test/")).toBe("https://app.example.test");
    expect(parseHostedOrigin("http://localhost:3000")).toBe("http://localhost:3000");
    expect(parseHostedOrigin("javascript:alert(1)")).toBeUndefined();
    expect(parseHostedOrigin(null)).toBeUndefined();
  });

  it("reports no page origin outside a browser", () => {
    vi.stubGlobal("window", undefined);
    expect(pageOrigin()).toBeUndefined();
  });
});
