#!/usr/bin/env node
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Static build step for the cloud marketing surface (CLOUD-005). After
// `vite build --mode web`, this writes the prerendered documents for the
// root and the four marketing pages, the client-only SPA fallback with the
// default no-index marker, robots.txt and sitemap.xml. Origin comes from
// PIDASH_PUBLIC_SITE_ORIGIN so a deployment can bake its own; the default
// is the hosted product address the spec names.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

const here = dirname(fileURLToPath(import.meta.url));
const appRoot = resolve(join(here, ".."));
/**
 * The build output directory. PIDASH_PRERENDER_OUT_DIR lets a test drive
 * the entry point against a scratch directory; deployments keep the
 * default.
 */
export function outputDirFrom(env) {
  const override = env.PIDASH_PRERENDER_OUT_DIR;
  return resolve(override === undefined || override === "" ? join(appRoot, "dist", "web") : override);
}

const distDir = outputDirFrom(process.env);

/**
 * Site origin for canonical addresses, the sitemap and robots.txt. Mirrors
 * `publicSiteOriginFromEnv` in src/core/platform/publicSiteOrigin.ts,
 * which bakes the same variable into the bundle: one env value must name
 * one origin in the served document and after hydration. Blank counts as
 * absent; trailing slashes are dropped.
 */
export const DEFAULT_SITE_ORIGIN = "https://pidash.airepublic.com";

export function siteOriginFromEnv(env) {
  const configured = env.PIDASH_PUBLIC_SITE_ORIGIN;
  const value = typeof configured === "string" ? configured.trim() : "";
  return (value === "" ? DEFAULT_SITE_ORIGIN : value).replace(/\/+$/, "");
}

const origin = siteOriginFromEnv(process.env);

/**
 * Whether this build writes the public marketing documents. The surface is
 * edition-owned (CLOUD-003–005): the OSS module declares no public root, so
 * a community build writes nothing and its routes keep 404ing.
 */
export function shouldPrerender(edition) {
  return edition?.routes?.some((route) => route.scope === "top-level" && route.path === "/") === true;
}

function escapeHtml(value) {
  return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
}

/** Replace the served head's marketing tags with a page's own metadata. */
export function injectHead(shell, page, siteOrigin) {
  const tags = [
    `<meta name="robots" content="index, follow" />`,
    `<link rel="canonical" href="${escapeHtml(page.canonical)}" />`,
    `<meta name="description" content="${escapeHtml(page.meta.description)}" />`,
    `<meta property="og:title" content="${escapeHtml(page.meta.title)}" />`,
    `<meta property="og:description" content="${escapeHtml(page.meta.description)}" />`,
    `<meta property="og:url" content="${escapeHtml(page.canonical)}" />`,
    `<meta property="og:image" content="${siteOrigin}/social-card.svg" />`,
    `<meta property="og:image:width" content="1200" />`,
    `<meta property="og:image:height" content="630" />`,
    `<meta property="og:image:alt" content="Pi Dash — projects, issues and agents in one place" />`,
    `<meta name="twitter:card" content="summary_large_image" />`,
    `<meta name="twitter:site" content="@airepublic" />`,
  ].join("\n    ");
  return shell
    .replace(/<title>[^<]*<\/title>/, `<title>${escapeHtml(page.meta.title)}</title>`)
    .replace(/\s*<meta name="description"[^>]*>/g, "")
    .replace(/\s*<meta property="og:[^"]+"[^>]*>/g, "")
    .replace(/\s*<meta name="twitter:[^"]+"[^>]*>/g, "")
    .replace(/\s*<link rel="canonical"[^>]*>/g, "")
    .replace(/(\s*)<\/head>/, `\n    ${tags}$1</head>`);
}

/** Put the static markup where the app mounts; React replaces it on load. */
export function injectMarkup(shell, markup) {
  return shell.replace('<div id="root"></div>', `<div id="root">${markup}</div>`);
}

/** Drop everything between the root mount and its matching close tag. */
function emptyRoot(html) {
  const open = html.indexOf('<div id="root">');
  if (open === -1) return html;
  const start = open + '<div id="root">'.length;
  let depth = 1;
  let index = start;
  while (index < html.length && depth > 0) {
    const nextOpen = html.indexOf("<div", index);
    const nextClose = html.indexOf("</div>", index);
    if (nextClose === -1) return html;
    if (nextOpen !== -1 && nextOpen < nextClose) {
      depth += 1;
      index = nextOpen + 4;
    } else {
      depth -= 1;
      index = nextClose + "</div>".length;
    }
  }
  return `${html.slice(0, start)}</div>${html.slice(index)}`;
}

export function spaFallback(shell) {
  return emptyRoot(
    shell
      .replace(/<title>[^<]*<\/title>/, "<title>Pi Dash</title>")
      .replace(/\s*<meta name="robots"[^>]*>/g, "")
      .replace(/\s*<link rel="canonical"[^>]*>/g, "")
      .replace(/(\s*)<\/head>/, `\n    <meta name="robots" content="noindex, nofollow" />$1</head>`)
  );
}

export function robotsTxt(siteOrigin) {
  return `User-agent: *\nAllow: /\n\nSitemap: ${siteOrigin}/sitemap.xml\n`;
}

export function sitemapXml(paths, siteOrigin) {
  const urls = paths.map((path) => `  <url><loc>${siteOrigin}${path}</loc></url>`).join("\n");
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`;
}

export function fileForPath(path) {
  const trimmed = path.replace(/^\/+/, "").replace(/\/+$/, "");
  return trimmed === "" ? "index.html" : `${trimmed}/index.html`;
}

/**
 * Write the built artifacts for the given pages into `outDir`: one document
 * per marketing address (its directory index), the client-only fallback and
 * the two crawler documents. Split from main so the output is testable
 * without running Vite.
 */
export function writeMarketingSite({ outDir, shell, pages, siteOrigin }) {
  for (const page of pages) {
    const html = injectMarkup(injectHead(shell, page, siteOrigin), page.markup);
    const file = join(outDir, fileForPath(page.path));
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, html);
  }
  writeFileSync(join(outDir, "__spa-fallback.html"), spaFallback(shell));
  writeFileSync(join(outDir, "robots.txt"), robotsTxt(siteOrigin));
  writeFileSync(join(outDir, "sitemap.xml"), sitemapXml(["/home", "/docs", "/pricing"], siteOrigin));
}

/**
 * Read the shell `vite build` wrote. Returns null (after reporting) when it
 * is absent; callers check this before doing expensive work so a build that
 * skipped Vite fails immediately instead of booting a module server to find
 * out the output directory is empty.
 */
export function readShell(outDir, fail) {
  try {
    return readFileSync(join(outDir, "index.html"), "utf8");
  } catch {
    fail(`[prerender] ${outDir}/index.html is missing; run \`vite build --mode web\` first.`);
    return null;
  }
}

/** Report a build failure the way the CLI does: stderr plus a failed exit code. */
function defaultFail(message) {
  console.error(message);
  process.exitCode = 1;
}

/**
 * The whole build step, with its environment injected so the flow is
 * testable without Vite: read the served shell, ask the edition whether it
 * owns the public surface, render the pages, then write them out.
 */
export async function runPrerender({
  outDir = distDir,
  siteOrigin = origin,
  loadEdition,
  loadPages,
  log = console.log,
  fail = defaultFail,
  shell,
} = {}) {
  const servedShell = shell === undefined ? readShell(outDir, fail) : shell;
  if (servedShell === null) return false;

  // The static surface belongs to the edition (CLOUD-003–005): the OSS
  // module declares none of it, so a community build writes no marketing
  // documents, robots or sitemap and its routes keep 404ing.
  if (!shouldPrerender(await loadEdition())) {
    log("[prerender] skipped: the edition declares no public root (OSS build)");
    return false;
  }
  const pages = await loadPages();
  writeMarketingSite({ outDir, shell: servedShell, pages, siteOrigin });
  log(`[prerender] wrote ${pages.length} marketing pages + robots.txt + sitemap.xml for ${siteOrigin}`);
  return true;
}

/**
 * Bring up a Vite module server, load the edition and the prerender entry
 * through it, and hand them to runPrerender. The server factory is a
 * parameter so the bootstrap (including its close) is testable without a
 * real Vite instance.
 */
export async function prerenderThroughServer({
  createViteServer = createServer,
  outDir,
  siteOrigin = origin,
  log = console.log,
  fail,
} = {}) {
  // Check the served shell before booting Vite: a build whose output is
  // missing has nothing to prerender into, and creating the module server
  // first turns that cheap failure into a slow one.
  const report = fail ?? defaultFail;
  const shell = readShell(resolve(outDir ?? distDir), report);
  if (shell === null) return false;

  const server = await createViteServer({
    root: appRoot,
    appType: "custom",
    server: { middlewareMode: true },
    logLevel: "error",
    optimizeDeps: { noDiscovery: true },
  });
  try {
    return await runPrerender({
      outDir,
      siteOrigin,
      log,
      fail: report,
      shell,
      loadEdition: async () => (await server.ssrLoadModule("/src/core/edition/index.ts")).edition,
      loadPages: async () =>
        (await server.ssrLoadModule("/src/features/marketing/prerender.tsx")).renderAllMarketingPages(siteOrigin),
    });
  } finally {
    await server.close();
  }
}

/** True when this module is the process entry point, not an import. */
export function isEntryPoint(argv1, moduleUrl) {
  return argv1 === fileURLToPath(moduleUrl);
}

export async function main(run = prerenderThroughServer) {
  await run();
}

// The CLI entry. `main` is injectable for the unit tests; the process env
// override lets the entry-point test run against a scratch output dir.
/* v8 ignore next 3 -- this module's own entry, exercised by the subprocess test */
if (isEntryPoint(process.argv[1], import.meta.url)) {
  await main();
}
