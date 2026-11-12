#!/usr/bin/env node
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Production static server for the parity `new` leg (NEWFRONT-1031). Serves
// a `vite build` output directory (dist/web by default) on the Playwright
// `new` base URL with the same same-origin topology as `pnpm dev`: /api
// and /auth reach the slot's API, /parity19-assets the slot's minio,
// /live the slot's live server, and static files follow the production
// nginx rule (`try_files $uri/index.html $uri /__spa-fallback.html`).
// Plain static files cannot fail on the dev server's
// on-demand transform, which is what flakes full-area proofs under host
// load ("Failed to fetch dynamically imported module"). Dev-server mode
// stays for local iteration; area gates build once and serve here.
//
// Usage (from apps/web_new, with the suite env exported):
//   pnpm parity:build-new   # vite build --mode web (rerun when sources change)
//   pnpm parity:serve-new    # this script, foreground; Ctrl-C stops it
//
// Environment: PARITY_NEW_PORT (default 3010), PARITY_NEW_DIST (web or
// desktop, default web), PIDASH_API_ORIGIN or PARITY_API_URL (default the
// parity API port), PARITY_MINIO_PORT and PARITY_LIVE_PORT when those
// services run, PARITY_NEW_ROOT only in the self-test (a scratch app
// root; gates never set it). All origins are plain http: parity
// backends are local.
import { createServer as createHttpServer, request as httpRequest } from "node:http";
import { existsSync, readFileSync, statSync } from "node:fs";
import { connect } from "node:net";
import { dirname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const appRoot = resolve(join(here, "..", "..", ".."));

/** The parity API port when neither PIDASH_API_ORIGIN nor PARITY_API_URL is set. */
export const DEFAULT_API_ORIGIN = "http://localhost:18019";

/** Default port: the Playwright `new` base URL. */
export const DEFAULT_NEW_PORT = 3010;

/** Blank counts as unset everywhere in this module. */
function pick(env, name) {
  const raw = env[name];
  if (raw === undefined) return null;
  const trimmed = raw.trim();
  return trimmed === "" ? null : trimmed;
}

/** Parse a TCP port or throw naming the variable. */
export function parsePort(raw, varName) {
  if (!/^[0-9]+$/.test(raw)) {
    throw new Error(`[parity:new] ${varName} must be a TCP port (1-65535), got ${JSON.stringify(raw)}.`);
  }
  const port = Number.parseInt(raw, 10);
  if (port < 1 || port > 65535) {
    throw new Error(`[parity:new] ${varName} must be a TCP port (1-65535), got ${JSON.stringify(raw)}.`);
  }
  return port;
}

/** The API origin: PIDASH_API_ORIGIN wins, then PARITY_API_URL, then the default. */
export function resolveApiOrigin(env) {
  const raw = pick(env, "PIDASH_API_ORIGIN") ?? pick(env, "PARITY_API_URL") ?? DEFAULT_API_ORIGIN;
  let url;
  try {
    url = new URL(raw);
  } catch {
    throw new Error(`[parity:new] API origin is not a URL, got ${JSON.stringify(raw)}.`);
  }
  if (url.protocol !== "http:" || url.username !== "" || url.password !== "") {
    throw new Error(`[parity:new] API origin must be plain http, got ${JSON.stringify(raw)}.`);
  }
  return `${url.protocol}//${url.host}`;
}

/** The build to serve: dist/web unless PARITY_NEW_DIST=desktop. */
export function resolveDistName(env) {
  const raw = pick(env, "PARITY_NEW_DIST") ?? "web";
  if (raw !== "web" && raw !== "desktop") {
    throw new Error(`[parity:new] PARITY_NEW_DIST must be "web" or "desktop", got ${JSON.stringify(raw)}.`);
  }
  return raw;
}

/**
 * Resolve the full server config. Throws naming the variable on any
 * invalid input, and when the build directory has no index.html (the
 * gate must run `pnpm parity:build-new` first).
 */
export function resolveConfig(env, root = appRoot) {
  const portRaw = pick(env, "PARITY_NEW_PORT");
  const port = portRaw === null ? DEFAULT_NEW_PORT : parsePort(portRaw, "PARITY_NEW_PORT");
  const distName = resolveDistName(env);
  const distDir = join(root, "dist", distName);
  const apiOrigin = resolveApiOrigin(env);
  const minioRaw = pick(env, "PARITY_MINIO_PORT");
  const minioOrigin = minioRaw === null ? null : `http://localhost:${parsePort(minioRaw, "PARITY_MINIO_PORT")}`;
  const liveRaw = pick(env, "PARITY_LIVE_PORT");
  const liveOrigin = liveRaw === null ? null : `http://localhost:${parsePort(liveRaw, "PARITY_LIVE_PORT")}`;
  if (!existsSync(join(distDir, "index.html"))) {
    throw new Error(`[parity:new] no build at ${distDir}; run \`pnpm parity:build-new\` first.`);
  }
  // The SPA fallback: the prerendered noindex shell when the build ran the
  // prerender step, else the app shell itself (a plain `vite build`).
  const prerendered = join(distDir, "__spa-fallback.html");
  const fallbackFile = existsSync(prerendered) ? prerendered : join(distDir, "index.html");
  return { port, distDir, apiOrigin, minioOrigin, liveOrigin, fallbackFile };
}

/**
 * The proxy target origin for a request path, or null to serve it from
 * the build directory. Mirrors the dev server's proxy table in
 * vite.config.ts: /api and /auth reach the API (the client owns
 * /auth/device, so that one serves the app shell), /parity19-assets the
 * slot's minio and /live the slot's live server when those are set.
 */
export function proxyTargetFor(pathname, config) {
  if (pathname === "/live" || pathname.startsWith("/live/")) return config.liveOrigin;
  if (pathname === "/parity19-assets" || pathname.startsWith("/parity19-assets/")) return config.minioOrigin;
  if (pathname === "/auth/device" || pathname === "/auth/device/") return null;
  if (pathname === "/auth" || pathname.startsWith("/auth/")) return config.apiOrigin;
  if (pathname === "/api" || pathname.startsWith("/api/")) return config.apiOrigin;
  return null;
}

/** Whether an upgrade (websocket) request tunnels to the target origin. */
export function upgradeTargetFor(pathname, config) {
  if (pathname === "/live" || pathname.startsWith("/live/")) return config.liveOrigin;
  if (pathname === "/api" || pathname.startsWith("/api/")) return config.apiOrigin;
  return null;
}

/**
 * Point proxied redirects back at this server. Native auth views answer
 * with absolute locations at the backend's configured web base; the
 * client follows them with fetch, which fails cross-origin. Only
 * backend-local (loopback) targets are rewritten; provider hand-offs to
 * external hosts and relative locations pass through untouched. Mirrors
 * the dev server's rewriteRedirectToDevOrigin in vite.config.ts.
 */
export function rewriteRedirect(location, reqHost) {
  let target;
  try {
    target = new URL(location);
  } catch {
    return location;
  }
  if (!["localhost", "127.0.0.1", "[::1]"].includes(target.hostname)) return location;
  if (typeof reqHost !== "string" || reqHost.length === 0 || target.host === reqHost) return location;
  target.host = reqHost;
  target.protocol = "http:";
  return target.toString();
}

const CONTENT_TYPES = {
  ".css": "text/css; charset=utf-8",
  ".html": "text/html; charset=utf-8",
  ".ico": "image/x-icon",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".map": "application/json; charset=utf-8",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".txt": "text/plain; charset=utf-8",
  ".webmanifest": "application/manifest+json",
  ".woff2": "font/woff2",
  ".xml": "application/xml",
};

/** Content type for a file name; unknown suffixes are opaque bytes. */
export function contentTypeFor(fileName) {
  const dot = fileName.lastIndexOf(".");
  const suffix = dot === -1 ? "" : fileName.slice(dot).toLowerCase();
  return CONTENT_TYPES[suffix] ?? "application/octet-stream";
}

/**
 * Cache policy: hashed /assets files are immutable for a year, everything
 * else must revalidate — parity runs rebuild often and index.html always
 * names the current hashes.
 */
export function cacheControlFor(pathname) {
  if (pathname === "/assets" || pathname.startsWith("/assets/")) return "public, max-age=31536000, immutable";
  return "no-store";
}

/**
 * Map a request path to a file under the build directory, mirroring the
 * production nginx rule `try_files $uri/index.html $uri
 * /__spa-fallback.html`: a directory index first (the prerendered
 * marketing pages), then the exact file (assets, robots, sitemap), then
 * the SPA fallback — even for suffixed misses, exactly like nginx. The
 * result always stays under the build directory: the URL parser strips
 * dot segments before this runs, and normalize() clamps any ".." a
 * direct caller passes at the root.
 */
export function staticFileFor(config, pathname) {
  const normalized = normalize(pathname);
  const dirIndex = join(config.distDir, `${normalized}${normalized.endsWith("/") ? "" : "/"}index.html`);
  if (existsSync(dirIndex) && statSync(dirIndex).isFile()) return dirIndex;
  const exact = join(config.distDir, normalized);
  if (existsSync(exact) && statSync(exact).isFile()) return exact;
  return config.fallbackFile;
}

function sendStatus(res, code, text) {
  res.writeHead(code, { "content-type": "text/plain; charset=utf-8", "content-length": Buffer.byteLength(text) });
  res.end(text);
}

function serveStatic(req, res, config, pathname) {
  if (req.method !== "GET" && req.method !== "HEAD") {
    sendStatus(res, 405, "method not allowed\n");
    return;
  }
  const file = staticFileFor(config, pathname);
  const body = readFileSync(file);
  res.writeHead(200, {
    "content-type": contentTypeFor(file),
    "content-length": body.length,
    "cache-control": cacheControlFor(pathname),
  });
  if (req.method === "HEAD") {
    res.end();
    return;
  }
  res.end(body);
}

function proxyHttp(req, res, target, rawPath) {
  const targetUrl = new URL(target);
  const outgoing = httpRequest(
    {
      host: targetUrl.hostname,
      port: targetUrl.port,
      method: req.method,
      path: rawPath,
      // Keep the incoming Host (changeOrigin false): backend presigned
      // URLs and auth views are addressed at this server's origin.
      headers: req.headers,
    },
    (incoming) => {
      const responseHeaders = { ...incoming.headers };
      const location = incoming.headers["location"];
      if (typeof location === "string") {
        responseHeaders["location"] = rewriteRedirect(location, req.headers.host);
      }
      res.writeHead(incoming.statusCode, responseHeaders);
      incoming.pipe(res);
    }
  );
  outgoing.on("error", (error) => {
    console.error(`[parity:new] proxy ${req.method} ${rawPath} -> ${target} failed: ${String(error)}`);
    if (!res.headersSent) sendStatus(res, 502, "bad gateway\n");
  });
  req.pipe(outgoing);
}

/** Tunnel one websocket upgrade to the target origin, headers untouched. */
export function tunnelUpgrade(req, socket, head, target) {
  const targetUrl = new URL(target);
  const upstream = connect({ host: targetUrl.hostname, port: Number(targetUrl.port) }, () => {
    const lines = [`${req.method} ${req.url} HTTP/${req.httpVersion}`];
    const raw = req.rawHeaders ?? [];
    for (let i = 0; i + 1 < raw.length; i += 2) lines.push(`${raw[i]}: ${raw[i + 1]}`);
    upstream.write(`${lines.join("\r\n")}\r\n\r\n`);
    if (head !== undefined && head.length > 0) upstream.write(head);
    socket.pipe(upstream);
    upstream.pipe(socket);
  });
  upstream.on("error", () => socket.destroy());
  socket.on("error", () => upstream.destroy());
}

/** Build the server; the caller listens. Exported for the self-test. */
export function createServer(config) {
  const server = createHttpServer((req, res) => {
    let pathname;
    try {
      pathname = new URL(req.url, "http://localhost").pathname;
    } catch {
      sendStatus(res, 400, "bad request\n");
      return;
    }
    const target = proxyTargetFor(pathname, config);
    if (target !== null) {
      proxyHttp(req, res, target, req.url);
      return;
    }
    serveStatic(req, res, config, pathname);
  });
  server.on("upgrade", (req, socket, head) => {
    let pathname;
    try {
      pathname = new URL(req.url, "http://localhost").pathname;
    } catch {
      socket.destroy();
      return;
    }
    const target = upgradeTargetFor(pathname, config);
    if (target === null) {
      socket.destroy();
      return;
    }
    tunnelUpgrade(req, socket, head, target);
  });
  server.on("clientError", (_error, socket) => socket.destroy());
  return server;
}

const invokedAsScript = process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (invokedAsScript) {
  let config;
  try {
    // PARITY_NEW_ROOT is a test hook (same idea as the prerender step's
    // PIDASH_PRERENDER_OUT_DIR): it lets the self-test drive the entry
    // point against a scratch directory. Gates never set it.
    const root = pick(process.env, "PARITY_NEW_ROOT") ?? appRoot;
    config = resolveConfig(process.env, root);
  } catch (error) {
    console.error(String(error));
    process.exit(2);
  }
  const server = createServer(config);
  server.on("error", (error) => {
    console.error(`[parity:new] cannot serve port ${config.port}: ${String(error)}`);
    process.exit(1);
  });
  server.listen(config.port, () => {
    console.log(
      `[parity:new] serving ${config.distDir} on http://localhost:${config.port}` +
        ` (api ${config.apiOrigin}, assets ${config.minioOrigin ?? "off"}, live ${config.liveOrigin ?? "off"})`
    );
  });
}
