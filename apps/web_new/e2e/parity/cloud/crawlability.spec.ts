// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-437): crawlability, row CLOUD-005
// (public pages are crawlable with stable metadata), green on the
// cloud-edition old app first. The same file must go green on
// apps/web_new once the cloud marketing area lands there.
//
// Observed behavior notes (cloud oracle): the marketing pages serve
// index/follow robots metadata, per-page canonical URLs, and social
// preview data; robots.txt points at the sitemap, which lists the
// home, docs, and pricing pages; the app shell skips the authenticated
// provider stack on static pages, so their content (including the
// single top-level heading) arrives in the first HTML bytes. These
// checks read raw HTTP responses — no browser session needed.
import { test, expect } from "../fixtures";
import { specTags, specTitle } from "../helpers/tags";

const SITE = "https://pidash.airepublic.com";

// Leg-aware fetch base (NEWFRONT-855): the Playwright config points
// baseURL at the oracle on --project=oracle and at PARITY_NEW_URL on
// --project=new, so fetching against it exercises the leg under test.
// A missing baseURL fails loudly: silently falling back to the oracle
// here would reintroduce the both-legs-fetch-oracle bug this replaced.
function legBase(baseURL: string | undefined): string {
  if (!baseURL) {
    throw new Error("[parity] crawlability: baseURL is not set; run through e2e/parity/playwright.config.ts");
  }
  return baseURL.replace(/\/+$/, "");
}

async function getText(base: string, path: string): Promise<{ status: number; body: string }> {
  const res = await fetch(`${base}${path}`);
  return { status: res.status, body: await res.text() };
}

test(
  specTitle(["CLOUD-005"], "robots points at a sitemap of the public pages"),
  { tag: specTags(["CLOUD-005"]) },
  async ({ baseURL }) => {
    const base = legBase(baseURL);
    const robots = await getText(base, "/robots.txt");
    expect(robots.status).toBe(200);
    expect(robots.body).toContain(`Sitemap: ${SITE}/sitemap.xml`);

    const sitemap = await getText(base, "/sitemap.xml");
    expect(sitemap.status).toBe(200);
    const locs = [...sitemap.body.matchAll(/<loc>([^<]+)<\/loc>/g)].map((match) => match[1]);
    expect(locs).toEqual([`${SITE}/home`, `${SITE}/docs`, `${SITE}/pricing`]);
  }
);

test(
  specTitle(["CLOUD-005"], "marketing pages carry crawler metadata and prerendered content"),
  { tag: specTags(["CLOUD-005"]) },
  async ({ baseURL }) => {
    const base = legBase(baseURL);
    for (const path of ["/home", "/docs", "/downloads", "/pricing"]) {
      const page = await getText(base, path);
      expect(page.status).toBe(200);
      expect(page.body).toContain('name="robots" content="index, follow"');
      expect(page.body).toContain(`rel="canonical" href="${SITE}${path}"`);
      expect(page.body).toContain(`property="og:url" content="${SITE}${path}"`);
      // One top-level heading in the first bytes: the static page content
      // prerendered without the authenticated provider stack.
      expect(page.body.match(/<h1[\s>]/g)?.length ?? 0).toBe(1);
    }
  }
);

test(
  specTitle(["CLOUD-005"], "root carries crawler metadata before its client redirect"),
  { tag: specTags(["CLOUD-005"]) },
  async ({ baseURL }) => {
    const root = await getText(legBase(baseURL), "/");
    expect(root.status).toBe(200);
    expect(root.body).toContain('name="robots" content="index, follow"');
    expect(root.body).toContain(`rel="canonical" href="${SITE}/home"`);
  }
);
