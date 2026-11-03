// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Card derivation (written from parity/specs/cloud.md §6, CLOUD-028):
// the installed and unfinished markers from the marketplace payload
// alone, the state-driven actions, the letter-tile initial, and the
// self-hosted-graphic to proxy to tile icon chain.
import { appIconUrl, type OpenhubApp } from "@pidash/api-client";

export type AppCardBadge = "installed" | "needs-connection" | null;

/** Installed from the payload alone — explicit flag or status, no extra fetch. */
export function isAppInstalled(app: OpenhubApp): boolean {
  return app.isInstalled === true || app.status === "installed";
}

/**
 * Installed but credentialless, exposing no tools at all: the
 * unfinished state comes from the service's own suggested action —
 * never derived locally — which names a connect leg.
 */
export function appNeedsConnection(app: OpenhubApp): boolean {
  return isAppInstalled(app) && typeof app.suggestedAction === "string" && app.suggestedAction.startsWith("connect");
}

/** The marker a card carries: unfinished wins over installed. */
export function appCardBadge(app: OpenhubApp): AppCardBadge {
  if (appNeedsConnection(app)) return "needs-connection";
  if (isAppInstalled(app)) return "installed";
  return null;
}

export type AppCardAction = "Install" | "Connect" | "Uninstall";

/**
 * Actions follow state: uninstalled cards offer installation,
 * unfinished ones connecting plus removal, connected ones removal
 * only.
 */
export function appCardActions(app: OpenhubApp): AppCardAction[] {
  if (!isAppInstalled(app)) return ["Install"];
  if (appNeedsConnection(app)) return ["Connect", "Uninstall"];
  return ["Uninstall"];
}

/** Letter-tile initial: the first letter of the name, uppercased. */
export function appLetter(name: string): string {
  const trimmed = name.trim();
  if (trimmed === "") return "?";
  return trimmed.slice(0, 1).toUpperCase();
}

/** Icon fallback stages: self-hosted graphic, proxy, letter tile. */
export type AppIconStage = "graphic" | "proxy" | "tile";

/** First stage: the self-hosted graphic, else the proxy, else the tile. */
export function firstIconStage(app: OpenhubApp): AppIconStage {
  if (app.iconSvgFallback !== undefined && app.iconSvgFallback !== "") return "graphic";
  if (app.iconUrl !== undefined && app.iconUrl !== "") return "proxy";
  return "tile";
}

/**
 * Next stage after a load failure: a failed graphic falls to the proxy
 * when the row carries an external address, anything else lands on the
 * always-rendered tile.
 */
export function nextIconStage(app: OpenhubApp, failed: AppIconStage): AppIconStage {
  if (failed === "graphic" && app.iconUrl !== undefined && app.iconUrl !== "") return "proxy";
  return "tile";
}

/**
 * Self-hosted graphic as an image source. Served as data, never parsed
 * as markup: an <img> cannot run the scripts an inline SVG could
 * carry.
 */
export function svgGraphicSource(svg: string): string {
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

/**
 * Image source for one stage, or null for the tile: the self-hosted
 * graphic first, then the proxied external address (never the raw
 * CDN URL).
 */
export function appIconSource(app: OpenhubApp, stage: AppIconStage, apiBase: string): string | null {
  if (stage === "graphic" && app.iconSvgFallback !== undefined && app.iconSvgFallback !== "") {
    return svgGraphicSource(app.iconSvgFallback);
  }
  if (stage === "proxy" && app.iconUrl !== undefined && app.iconUrl !== "") {
    return appIconUrl(apiBase, app.iconUrl);
  }
  return null;
}
