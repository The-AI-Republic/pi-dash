// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import type { OpenhubApp } from "@pidash/api-client";
import { describe, expect, it } from "vitest";

import {
  appCardActions,
  appCardBadge,
  appIconSource,
  appLetter,
  appNeedsConnection,
  firstIconStage,
  isAppInstalled,
  nextIconStage,
  svgGraphicSource,
} from "./lib/cards.js";

function card(overrides: Partial<OpenhubApp> = {}): OpenhubApp {
  return { appId: "app-fresh", slug: "fresh", name: "Fresh App", ...overrides };
}

describe("isAppInstalled / appNeedsConnection / appCardBadge", () => {
  it("reads installed from the flag or the status, unfinished from the suggested action (CLOUD-028)", () => {
    const fresh = card();
    expect(isAppInstalled(fresh)).toBe(false);
    expect(appNeedsConnection(fresh)).toBe(false);
    expect(appCardBadge(fresh)).toBeNull();

    const pending = card({ isInstalled: true, suggestedAction: "connect_auth" });
    expect(isAppInstalled(pending)).toBe(true);
    expect(appNeedsConnection(pending)).toBe(true);
    expect(appCardBadge(pending)).toBe("needs-connection");

    const ready = card({ isInstalled: true, suggestedAction: "none" });
    expect(isAppInstalled(ready)).toBe(true);
    expect(appNeedsConnection(ready)).toBe(false);
    expect(appCardBadge(ready)).toBe("installed");

    expect(appCardBadge(card({ status: "installed", suggestedAction: "none" }))).toBe("installed");
    expect(appCardBadge(card({ isInstalled: true, suggestedAction: "connect_oauth" }))).toBe("needs-connection");
    expect(appCardBadge(card({ isInstalled: true }))).toBe("installed");
    expect(appCardBadge(card({ suggestedAction: "connect_auth" }))).toBeNull();
  });
});

describe("appCardActions", () => {
  it("offers Install, Connect+Uninstall, or Uninstall by state (CLOUD-028)", () => {
    expect(appCardActions(card())).toEqual(["Install"]);
    expect(appCardActions(card({ isInstalled: true, suggestedAction: "connect_auth" }))).toEqual([
      "Connect",
      "Uninstall",
    ]);
    expect(appCardActions(card({ isInstalled: true, suggestedAction: "none" }))).toEqual(["Uninstall"]);
    expect(appCardActions(card({ status: "installed" }))).toEqual(["Uninstall"]);
  });
});

describe("appLetter", () => {
  it("tiles the first letter of the name (CLOUD-028)", () => {
    expect(appLetter("Fresh App")).toBe("F");
    expect(appLetter("  padded")).toBe("P");
    expect(appLetter("lower")).toBe("L");
    expect(appLetter("")).toBe("?");
    expect(appLetter("   ")).toBe("?");
  });
});

describe("firstIconStage / nextIconStage", () => {
  const svg = '<svg xmlns="http://www.w3.org/2000/svg"></svg>';

  it("prefers the graphic, then the proxy, then the tile (CLOUD-028)", () => {
    expect(firstIconStage(card({ iconSvgFallback: svg, iconUrl: "https://cdn.example.com/ignored.png" }))).toBe(
      "graphic"
    );
    expect(firstIconStage(card({ iconUrl: "https://cdn.example.com/icon.png" }))).toBe("proxy");
    expect(firstIconStage(card())).toBe("tile");
    expect(firstIconStage(card({ iconSvgFallback: "", iconUrl: "" }))).toBe("tile");
    expect(firstIconStage(card({ iconSvgFallback: "", iconUrl: "https://cdn.example.com/icon.png" }))).toBe("proxy");
  });

  it("falls from the graphic to the proxy, then lands on the tile (CLOUD-028)", () => {
    const both = card({ iconSvgFallback: svg, iconUrl: "https://cdn.example.com/icon.png" });
    expect(nextIconStage(both, "graphic")).toBe("proxy");
    expect(nextIconStage(both, "proxy")).toBe("tile");
    expect(nextIconStage(card({ iconSvgFallback: svg }), "graphic")).toBe("tile");
    expect(nextIconStage(card({ iconSvgFallback: svg, iconUrl: "" }), "graphic")).toBe("tile");
    expect(nextIconStage(card(), "tile")).toBe("tile");
  });
});

describe("svgGraphicSource / appIconSource", () => {
  const svg = '<svg xmlns="http://www.w3.org/2000/svg"><rect width="32" height="32"/></svg>';

  it("serves the self-hosted graphic as script-free data (CLOUD-028)", () => {
    const source = svgGraphicSource(svg);
    expect(source.startsWith("data:image/svg+xml,")).toBe(true);
    expect(decodeURIComponent(source.slice("data:image/svg+xml,".length))).toBe(svg);
  });

  it("resolves each stage to its source, proxying external addresses (CLOUD-028)", () => {
    const app = card({ iconSvgFallback: svg, iconUrl: "https://cdn.example.com/icon.png" });
    expect(appIconSource(app, "graphic", "https://api.test")).toBe(svgGraphicSource(svg));
    expect(appIconSource(app, "proxy", "https://api.test")).toBe(
      "https://api.test/api/openhub/apps/icon/?url=https%3A%2F%2Fcdn.example.com%2Ficon.png"
    );
    expect(appIconSource(app, "tile", "https://api.test")).toBeNull();
    expect(appIconSource(card(), "graphic", "https://api.test")).toBeNull();
    expect(appIconSource(card(), "proxy", "https://api.test")).toBeNull();
    expect(appIconSource(card({ iconSvgFallback: "" }), "graphic", "https://api.test")).toBeNull();
    expect(appIconSource(card({ iconUrl: "" }), "proxy", "https://api.test")).toBeNull();
  });
});
