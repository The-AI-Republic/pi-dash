// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Prerender entry for the public marketing pages (CLOUD-005). The build
// script loads this module through Vite, renders each page to static
// markup with the real frame and sections, and injects the crawler head.
// It deliberately composes the page components directly (not through the
// router) so no session stores, query client or browser APIs are needed.
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { MarketingFrame } from "./components/MarketingFrame.js";
import { MarketingHome } from "./components/MarketingHome.js";
import { MarketingIntro } from "./components/MarketingIntro.js";
import { metaFor, PUBLIC_SITE_ORIGIN, type MarketingPageMeta } from "./content.js";

export interface PrerenderedPage {
  path: string;
  markup: string;
  meta: MarketingPageMeta;
  canonical: string;
}

/** The static pages the cloud build writes, in sitemap order plus root. */
export const PRERENDER_PATHS = ["/", "/home", "/docs", "/downloads", "/pricing"] as const;

function PageBody({ path }: { path: string }): React.ReactElement {
  if (path === "/home" || path === "/") return <MarketingHome />;
  if (path === "/docs" || path === "/downloads" || path === "/pricing") {
    return <MarketingIntro path={path} />;
  }
  throw new Error(`No prerender body for ${path}`);
}

export function canonicalFor(path: string, origin: string = PUBLIC_SITE_ORIGIN): string {
  // The signed-out root is a forward to the marketing home; its canonical
  // address is the home page (CLOUD-005 root scenario).
  return `${origin.replace(/\/+$/, "")}${path === "/" ? "/home" : path}`;
}

export function renderMarketingPage(path: string, origin: string = PUBLIC_SITE_ORIGIN): PrerenderedPage {
  const meta = path === "/" ? metaFor("/home") : metaFor(path);
  return {
    path,
    meta,
    canonical: canonicalFor(path, origin),
    markup: renderToStaticMarkup(
      <MarketingFrame>
        <PageBody path={path} />
      </MarketingFrame>
    ),
  };
}

export function renderAllMarketingPages(origin: string = PUBLIC_SITE_ORIGIN): PrerenderedPage[] {
  return PRERENDER_PATHS.map((path) => renderMarketingPage(path, origin));
}
