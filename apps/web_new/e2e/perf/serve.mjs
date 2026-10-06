// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Static bundle server for performance measurement (NEWFRONT-21). Serves a
// production build directory as the app origin: /api and /auth proxy to a
// Django backend when --api is given (baseline measurement); hashed
// /assets/* files are immutable, like production caching, so warm loads
// measure cache hits; index.html and the SPA fallback are no-cache.
// No dependencies; shared by the manual baseline runs and the CI perf
// specs (via playwright.perf.config.ts webServer).
import { createServer, request as httpRequest } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize, sep } from "node:path";

const CONTENT_TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
  ".webmanifest": "application/manifest+json",
};

function parseArgs(argv) {
  const out = { dir: "", port: 0, api: "" };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = argv[i + 1] ?? "";
    if (flag === "--dir") out.dir = value;
    else if (flag === "--port") out.port = Number(value);
    else if (flag === "--api") out.api = value.replace(/\/+$/, "");
    else {
      throw new Error(`unknown flag: ${flag}`);
    }
    i++;
  }
  if (!out.dir || !Number.isInteger(out.port) || out.port <= 0) {
    throw new Error("usage: serve.mjs --dir <build-dir> --port <port> [--api <django-origin>]");
  }
  return out;
}

function proxyToApi(api, req, res) {
  const target = new URL(req.url ?? "/", api);
  const proxyReq = httpRequest(
    {
      protocol: target.protocol,
      hostname: target.hostname,
      port: target.port,
      path: `${target.pathname}${target.search}`,
      method: req.method,
      headers: { ...req.headers, host: target.host },
    },
    (proxyRes) => {
      res.writeHead(proxyRes.statusCode ?? 502, proxyRes.headers);
      proxyRes.pipe(res);
    }
  );
  proxyReq.on("error", () => {
    if (!res.headersSent) {
      res.writeHead(502, { "content-type": "application/json" });
    }
    res.end(JSON.stringify({ error: "api proxy failed" }));
  });
  req.pipe(proxyReq);
}

export function createBundleServer({ dir, api }) {
  return createServer(async (req, res) => {
    try {
      const url = new URL(req.url ?? "/", "http://localhost");
      if (url.pathname === "/api" || url.pathname.startsWith("/api/") || url.pathname.startsWith("/auth/")) {
        if (!api) {
          res.writeHead(502, { "content-type": "application/json" });
          res.end(JSON.stringify({ error: "no backend: start with --api or mock the route" }));
          return;
        }
        proxyToApi(api, req, res);
        return;
      }
      const safe = normalize(url.pathname).replace(/^(\.\.[/\\])+/, "");
      let file = join(dir, safe === "/" ? "index.html" : safe.slice(1));
      const within = file === dir || file.startsWith(`${dir}${sep}`);
      let found = false;
      if (within) {
        try {
          found = (await stat(file)).isFile();
        } catch {
          found = false;
        }
      }
      if (!found && extname(safe) === "") {
        // Client-side route: serve the shell like production rewrites do.
        file = join(dir, "index.html");
        found = true;
      }
      if (!found) {
        res.writeHead(404, { "content-type": "text/plain" });
        res.end("not found");
        return;
      }
      const body = await readFile(file);
      const immutable = url.pathname.startsWith("/assets/");
      res.writeHead(200, {
        "content-type": CONTENT_TYPES[extname(file)] ?? "application/octet-stream",
        "cache-control": immutable ? "public, max-age=31536000, immutable" : "no-cache",
        "content-length": body.length,
      });
      res.end(body);
    } catch {
      if (!res.headersSent) {
        res.writeHead(500, { "content-type": "text/plain" });
      }
      res.end("server error");
    }
  });
}

const invokedDirectly = process.argv[1] !== undefined && import.meta.url.endsWith("/serve.mjs");
if (invokedDirectly && process.argv[1].endsWith("serve.mjs")) {
  const options = parseArgs(process.argv.slice(2));
  try {
    await stat(join(options.dir, "index.html"));
  } catch {
    console.error(`[perf] ${options.dir} has no index.html; build the bundle first.`);
    process.exit(1);
  }
  const server = createBundleServer(options);
  server.listen(options.port, () => {
    console.log(`[perf] serving ${options.dir} on http://localhost:${options.port}`);
  });
}
