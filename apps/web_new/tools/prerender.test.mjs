// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Static-build helpers for the marketing surface (CLOUD-005). Every
// assertion reads the emitted document text the way a crawler does.
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PRERENDER_ENTRY } from "./prerender.test-paths.mjs";
import { publicSiteOriginFromEnv } from "../src/core/platform/publicSiteOrigin.ts";

import {
  fileForPath,
  injectHead,
  injectMarkup,
  robotsTxt,
  shouldPrerender,
  siteOriginFromEnv,
  sitemapXml,
  isEntryPoint,
  main,
  outputDirFrom,
  prerenderThroughServer,
  readShell,
  runPrerender,
  spaFallback,
  writeMarketingSite,
} from "./prerender.mjs";

const SHELL = `<!doctype html><html><head><title>Pi Dash</title>
    <meta name="description" content="served" />
    <meta property="og:title" content="served" />
    <meta property="og:image" content="/social-card.svg" />
    <meta name="twitter:card" content="summary_large_image" />
  </head><body><div id="root"></div></body></html>`;

const PAGE = {
  path: "/home",
  canonical: "https://site.test/home",
  meta: { title: "Home & co", description: 'A "quoted" <description>' },
};

describe("siteOriginFromEnv", () => {
  it("defaults to the hosted address, takes a deployment origin and drops trailing slashes", () => {
    expect(siteOriginFromEnv({})).toBe("https://pidash.airepublic.com");
    expect(siteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: "" })).toBe("https://pidash.airepublic.com");
    expect(siteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: "https://example.org//" })).toBe("https://example.org");
    expect(siteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: "  https://example.org/  " })).toBe("https://example.org");
  });

  it("matches the rule the bundle bakes, so served and hydrated metadata agree", () => {
    for (const value of [undefined, "", "   ", "https://example.org", "https://example.org//", " https://x.test/ "]) {
      expect(siteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: value })).toBe(
        publicSiteOriginFromEnv({ PIDASH_PUBLIC_SITE_ORIGIN: value })
      );
    }
  });
});

describe("shouldPrerender", () => {
  it("writes the surface only when the edition claims the public root", () => {
    expect(shouldPrerender({ id: "cloud", routes: [{ path: "/", scope: "top-level" }] })).toBe(true);
    expect(shouldPrerender({ id: "oss", routes: [] })).toBe(false);
    expect(shouldPrerender({ id: "oss" })).toBe(false);
    expect(shouldPrerender(undefined)).toBe(false);
  });

  it("does not accept a workspace route that happens to be named /", () => {
    expect(shouldPrerender({ id: "x", routes: [{ path: "/" }] })).toBe(false);
  });
});

describe("head injection", () => {
  it("replaces the served marketing tags with page metadata", () => {
    const html = injectHead(SHELL, PAGE, "https://site.test");
    expect(html).toContain("<title>Home &amp; co</title>");
    expect(html).toContain('<meta name="robots" content="index, follow" />');
    expect(html).toContain('<link rel="canonical" href="https://site.test/home" />');
    expect(html).toContain('<meta property="og:url" content="https://site.test/home" />');
    expect(html).toContain('<meta property="og:image:width" content="1200" />');
    expect(html).toContain('<meta name="twitter:site" content="@airepublic" />');
    expect(html).toContain('<meta name="description" content="A &quot;quoted&quot; &lt;description&gt;" />');
    expect(html.match(/og:title/g)).toHaveLength(1);
    expect(html.match(/name="description"/g)).toHaveLength(1);
    // The shell ships its own twitter:card; replacing, not appending to it,
    // keeps the served document identical to what the route head emits
    // after hydration (CLOUD-005).
    expect(html.match(/name="twitter:card"/g)).toHaveLength(1);
    expect(html.match(/name="twitter:site"/g)).toHaveLength(1);
    expect(html).toContain('<meta property="og:image" content="https://site.test/social-card.svg" />');
  });
});

describe("document assembly", () => {
  it("fills the mount point with the prerendered markup", () => {
    expect(injectMarkup(SHELL, "<main><h1>Hello</h1></main>")).toContain(
      '<div id="root"><main><h1>Hello</h1></main></div>'
    );
  });

  it("strips page metadata from the client-only fallback and marks it no-index", () => {
    const prerendered = injectMarkup(injectHead(SHELL, PAGE, "https://site.test"), "<main>prerendered</main>");
    const fallback = spaFallback(prerendered);
    expect(fallback).toContain("<title>Pi Dash</title>");
    expect(fallback).toContain('<meta name="robots" content="noindex, nofollow" />');
    expect(fallback).not.toContain('rel="canonical"');
    expect(fallback).not.toContain("prerendered");
    expect(fallback).toContain('<div id="root"></div>');
  });

  it("keeps a document without a root mount intact apart from the head markers", () => {
    const shell = "<!doctype html><html><head></head><body><p>plain</p></body></html>";
    const fallback = spaFallback(shell);
    expect(fallback).toContain("<body><p>plain</p></body>");
    expect(fallback).toContain('<meta name="robots" content="noindex, nofollow" />');
  });

  it("leaves an unclosed root mount alone", () => {
    const shell = '<!doctype html><html><body><div id="root"><div>open</body></html>';
    expect(spaFallback(shell)).toContain('<div id="root"><div>open');
  });

  it("empties the mount point even when the markup nests divs", () => {
    const nested = injectMarkup(SHELL, '<div class="a"><div>b</div></div><p>after</p>');
    expect(spaFallback(nested)).toContain('<div id="root"></div></body>');
    expect(spaFallback(nested)).not.toContain("after");
  });
});

describe("crawl documents", () => {
  it("points robots at the sitemap", () => {
    expect(robotsTxt("https://site.test")).toBe("User-agent: *\nAllow: /\n\nSitemap: https://site.test/sitemap.xml\n");
  });

  it("lists the public pages in order", () => {
    const xml = sitemapXml(["/home", "/docs", "/pricing"], "https://site.test");
    expect([...xml.matchAll(/<loc>([^<]+)<\/loc>/g)].map((match) => match[1])).toEqual([
      "https://site.test/home",
      "https://site.test/docs",
      "https://site.test/pricing",
    ]);
    expect(xml.startsWith('<?xml version="1.0" encoding="UTF-8"?>')).toBe(true);
  });

  it("maps each address to a directory index", () => {
    expect(fileForPath("/")).toBe("index.html");
    expect(fileForPath("/home")).toBe("home/index.html");
    expect(fileForPath("/docs/")).toBe("docs/index.html");
    expect(fileForPath("//")).toBe("index.html");
  });
});

function pages() {
  return [
    ["/", "Home", "home"],
    ["/home", "Home", "home"],
    ["/docs", "Docs", "docs"],
    ["/downloads", "Downloads", "downloads"],
    ["/pricing", "Pricing", "pricing"],
  ].map(([path, title, slug]) => ({
    path,
    canonical: `https://site.test/${slug}`,
    meta: { title, description: `${title} page` },
    markup: `<main><h1>${title}</h1></main>`,
  }));
}

describe("site output", () => {
  const created = [];

  afterEach(() => {
    while (created.length > 0) rmSync(created.pop(), { recursive: true, force: true });
  });

  function scratchDir() {
    const dir = mkdtempSync(join(tmpdir(), "prerender-test-"));
    created.push(dir);
    return dir;
  }

  it("writes each page as a directory index plus the fallback and crawler documents", () => {
    const outDir = scratchDir();
    writeMarketingSite({ outDir, shell: SHELL, pages: pages(), siteOrigin: "https://site.test" });

    for (const [path, title] of [
      ["index.html", "Home"],
      ["home/index.html", "Home"],
      ["docs/index.html", "Docs"],
      ["downloads/index.html", "Downloads"],
      ["pricing/index.html", "Pricing"],
    ]) {
      const html = readFileSync(join(outDir, path), "utf8");
      expect(html).toContain(`<h1>${title}</h1>`);
      expect(html).toContain('<meta name="robots" content="index, follow" />');
    }
    expect(readFileSync(join(outDir, "robots.txt"), "utf8")).toContain("Sitemap: https://site.test/sitemap.xml");
    const sitemap = readFileSync(join(outDir, "sitemap.xml"), "utf8");
    expect([...sitemap.matchAll(/<loc>([^<]+)<\/loc>/g)].map((match) => match[1])).toEqual([
      "https://site.test/home",
      "https://site.test/docs",
      "https://site.test/pricing",
    ]);
    expect(readFileSync(join(outDir, "__spa-fallback.html"), "utf8")).toContain(
      '<meta name="robots" content="noindex, nofollow" />'
    );
    // Each emitted document carries one copy of every tag the hydrated
    // route also writes, so served and client head agree (CLOUD-005).
    for (const path of ["home/index.html", "docs/index.html", "downloads/index.html", "pricing/index.html"]) {
      const html = readFileSync(join(outDir, path), "utf8");
      expect(html.match(/name="twitter:card"/g)).toHaveLength(1);
      expect(html.match(/name="twitter:site"/g)).toHaveLength(1);
      expect(html.match(/property="og:image"/g)).toHaveLength(1);
      expect(html.match(/rel="canonical"/g)).toHaveLength(1);
    }
  });

  it("takes the page canonical from the rendered page and the preview image from the site origin", () => {
    const outDir = scratchDir();
    writeMarketingSite({ outDir, shell: SHELL, pages: pages(), siteOrigin: "https://other.test" });
    const home = readFileSync(join(outDir, "home/index.html"), "utf8");
    expect(home).toContain('rel="canonical" href="https://site.test/home"');
    expect(home).toContain('content="https://other.test/social-card.svg"');
  });
});

describe("runPrerender", () => {
  const created = [];

  afterEach(() => {
    while (created.length > 0) rmSync(created.pop(), { recursive: true, force: true });
  });

  function scratchDir(withShell = true) {
    const dir = mkdtempSync(join(tmpdir(), "prerender-run-"));
    created.push(dir);
    if (withShell) writeFileSync(join(dir, "index.html"), SHELL);
    return dir;
  }

  const cloud = { id: "cloud", routes: [{ path: "/", scope: "top-level" }] };
  const oss = { id: "oss", routes: [] };

  it("writes nothing and fails when the served shell is missing", async () => {
    const fail = vi.fn();
    const wrote = await runPrerender({
      outDir: scratchDir(false),
      loadEdition: async () => cloud,
      loadPages: async () => pages(),
      log: vi.fn(),
      fail,
    });
    expect(wrote).toBe(false);
    expect(fail).toHaveBeenCalledWith(expect.stringContaining("run `vite build --mode web` first"));
  });

  it("skips the whole surface on a community build", async () => {
    const outDir = scratchDir();
    const log = vi.fn();
    const loadPages = vi.fn();
    const wrote = await runPrerender({ outDir, loadEdition: async () => oss, loadPages, log });
    expect(wrote).toBe(false);
    expect(loadPages).not.toHaveBeenCalled();
    expect(log).toHaveBeenCalledWith("[prerender] skipped: the edition declares no public root (OSS build)");
    expect(() => readFileSync(join(outDir, "robots.txt"))).toThrow();
  });

  it("writes the full surface on the cloud edition", async () => {
    const outDir = scratchDir();
    const log = vi.fn();
    const wrote = await runPrerender({
      outDir,
      siteOrigin: "https://site.test",
      loadEdition: async () => cloud,
      loadPages: async () => pages(),
      log,
    });
    expect(wrote).toBe(true);
    expect(readFileSync(join(outDir, "robots.txt"), "utf8")).toContain("https://site.test/sitemap.xml");
    expect(readFileSync(join(outDir, "docs/index.html"), "utf8")).toContain("<h1>Docs</h1>");
    expect(log).toHaveBeenCalledWith(
      "[prerender] wrote 5 marketing pages + robots.txt + sitemap.xml for https://site.test"
    );
  });
});

describe("prerenderThroughServer", () => {
  const created = [];

  afterEach(() => {
    while (created.length > 0) rmSync(created.pop(), { recursive: true, force: true });
  });

  function scratchDir() {
    const dir = mkdtempSync(join(tmpdir(), "prerender-server-"));
    created.push(dir);
    writeFileSync(join(dir, "index.html"), SHELL);
    return dir;
  }

  function fakeServer(edition, close) {
    return async () => ({
      ssrLoadModule: async (path) => {
        if (path === "/src/core/edition/index.ts") return { edition };
        return { renderAllMarketingPages: () => pages() };
      },
      close,
    });
  }

  it("loads the edition and pages through the server and closes it", async () => {
    const outDir = scratchDir();
    const close = vi.fn();
    const wrote = await prerenderThroughServer({
      createViteServer: fakeServer({ id: "cloud", routes: [{ path: "/", scope: "top-level" }] }, close),
      outDir,
      siteOrigin: "https://site.test",
      log: vi.fn(),
    });
    expect(wrote).toBe(true);
    expect(readFileSync(join(outDir, "sitemap.xml"), "utf8")).toContain("https://site.test/home");
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("closes the server and skips when the edition is community", async () => {
    const outDir = scratchDir();
    const close = vi.fn();
    const wrote = await prerenderThroughServer({
      createViteServer: fakeServer({ id: "oss", routes: [] }, close),
      outDir,
      log: vi.fn(),
    });
    expect(wrote).toBe(false);
    expect(close).toHaveBeenCalledTimes(1);
    expect(() => readFileSync(join(outDir, "robots.txt"))).toThrow();
  });

  it("reports a missing shell without creating the Vite server", async () => {
    const createViteServer = vi.fn();
    const fail = vi.fn();
    const outDir = mkdtempSync(join(tmpdir(), "prerender-server-empty-"));
    created.push(outDir);
    const wrote = await prerenderThroughServer({ createViteServer, outDir, fail, log: vi.fn() });
    expect(wrote).toBe(false);
    expect(createViteServer).not.toHaveBeenCalled();
    expect(fail).toHaveBeenCalledWith(expect.stringContaining(`[prerender] ${outDir}/index.html is missing`));
  });

  it("uses the default failure reporter when none is injected", async () => {
    const createViteServer = vi.fn();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const previous = process.exitCode;
    process.exitCode = undefined;
    const outDir = mkdtempSync(join(tmpdir(), "prerender-server-default-"));
    created.push(outDir);
    try {
      const wrote = await prerenderThroughServer({ createViteServer, outDir, log: vi.fn() });
      expect(wrote).toBe(false);
      expect(createViteServer).not.toHaveBeenCalled();
      expect(error).toHaveBeenCalledWith(
        `[prerender] ${outDir}/index.html is missing; run \`vite build --mode web\` first.`
      );
      expect(process.exitCode).toBe(1);
    } finally {
      process.exitCode = previous;
      error.mockRestore();
    }
  });
});

describe("readShell", () => {
  it("returns the served document and reports nothing", () => {
    const outDir = mkdtempSync(join(tmpdir(), "prerender-shell-"));
    try {
      writeFileSync(join(outDir, "index.html"), SHELL);
      const fail = vi.fn();
      expect(readShell(outDir, fail)).toBe(SHELL);
      expect(fail).not.toHaveBeenCalled();
    } finally {
      rmSync(outDir, { recursive: true, force: true });
    }
  });

  it("returns null after reporting when the output directory is empty", () => {
    const outDir = mkdtempSync(join(tmpdir(), "prerender-shell-missing-"));
    try {
      const fail = vi.fn();
      expect(readShell(outDir, fail)).toBeNull();
      expect(fail).toHaveBeenCalledWith(
        `[prerender] ${outDir}/index.html is missing; run \`vite build --mode web\` first.`
      );
    } finally {
      rmSync(outDir, { recursive: true, force: true });
    }
  });
});

describe("isEntryPoint", () => {
  it("recognizes the module's own file URL and nothing else", () => {
    expect(
      isEntryPoint(
        fileURLToPath(new URL("./prerender.mjs", import.meta.url)),
        new URL("./prerender.mjs", import.meta.url)
      )
    ).toBe(true);
    expect(isEntryPoint("/some/other/file.mjs", new URL("./prerender.mjs", import.meta.url))).toBe(false);
  });
});

describe("entry point", () => {
  it("runs the injected build step", async () => {
    const run = vi.fn(async () => true);
    await main(run);
    expect(run).toHaveBeenCalledTimes(1);
  });

  it("fails with a message and exit 1 when the served shell is missing", () => {
    const outDir = mkdtempSync(join(tmpdir(), "prerender-entry-"));
    try {
      const result = spawnSync(process.execPath, [PRERENDER_ENTRY], {
        env: { ...process.env, PIDASH_PRERENDER_OUT_DIR: outDir },
        encoding: "utf8",
      });
      expect(result.status).toBe(1);
      expect(result.stderr).toContain(`[prerender] ${outDir}/index.html is missing`);
    } finally {
      rmSync(outDir, { recursive: true, force: true });
    }
  });
});

describe("default failure reporter", () => {
  it("prints the message and marks the process failed", async () => {
    const outDir = mkdtempSync(join(tmpdir(), "prerender-fail-"));
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const previous = process.exitCode;
    process.exitCode = undefined;
    try {
      const wrote = await runPrerender({ outDir, loadEdition: async () => ({ id: "oss", routes: [] }) });
      expect(wrote).toBe(false);
      expect(error).toHaveBeenCalledWith(expect.stringContaining("is missing"));
      expect(process.exitCode).toBe(1);
    } finally {
      process.exitCode = previous;
      error.mockRestore();
      rmSync(outDir, { recursive: true, force: true });
    }
  });
});

describe("outputDirFrom", () => {
  it("defaults to the web build output", () => {
    expect(outputDirFrom({})).toMatch(/apps\/web_new\/dist\/web$/);
    expect(outputDirFrom({ PIDASH_PRERENDER_OUT_DIR: "" })).toMatch(/apps\/web_new\/dist\/web$/);
  });

  it("resolves an explicit override", () => {
    expect(outputDirFrom({ PIDASH_PRERENDER_OUT_DIR: "/scratch/out" })).toBe("/scratch/out");
  });
});
