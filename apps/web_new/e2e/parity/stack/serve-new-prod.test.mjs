// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Self-test for the parity `new` production server (NEWFRONT-1031; run:
// pnpm --filter web_new test:parity-stack). Pure-function tables pin the
// env resolution, routing, redirect rewrite and file mapping; ephemeral
// localhost servers prove the static serving, proxying and websocket
// tunneling; subprocess runs prove the entry point. No fixed ports, no
// network beyond loopback.
import { strict as assert } from "node:assert";
import { spawn } from "node:child_process";
import { createServer as createHttpServer, request as httpRequest } from "node:http";
import { mkdtempSync, mkdirSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, createServer as createTcpServer } from "node:net";
import { describe, it, after } from "node:test";
import { fileURLToPath } from "node:url";
import { dirname } from "node:path";
import {
  DEFAULT_API_ORIGIN,
  DEFAULT_NEW_PORT,
  cacheControlFor,
  contentTypeFor,
  createServer as createProdServer,
  parsePort,
  proxyTargetFor,
  resolveApiOrigin,
  resolveConfig,
  resolveDistName,
  rewriteRedirect,
  staticFileFor,
  tunnelUpgrade,
  upgradeTargetFor,
} from "./serve-new-prod.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const script = join(here, "serve-new-prod.mjs");

/** Every server opened here, closed after the run. */
const closables = [];
after(() => {
  for (const closeable of closables.splice(0)) {
    try {
      closeable.close();
    } catch {}
  }
});

function track(server) {
  closables.push(server);
  return server;
}

/**
 * A scratch app root with dist/web/index.html, one hashed asset, one
 * prerendered marketing page (home/index.html), an empty subdir, a
 * directory shaped like a file index (xdir/index.html/), and — unless
 * asked otherwise — the prerendered SPA fallback shell.
 */
function makeDist({ withFallback = true } = {}) {
  const root = mkdtempSync(join(tmpdir(), "parity-new-prod-"));
  const distDir = join(root, "dist", "web");
  mkdirSync(join(distDir, "assets"), { recursive: true });
  mkdirSync(join(distDir, "subdir"), { recursive: true });
  mkdirSync(join(distDir, "home"), { recursive: true });
  mkdirSync(join(distDir, "xdir", "index.html"), { recursive: true });
  writeFileSync(join(distDir, "index.html"), "<!doctype html><title>test-index</title>\n");
  writeFileSync(join(distDir, "home", "index.html"), "<!doctype html><title>test-home</title>\n");
  writeFileSync(join(distDir, "assets", "app-abc123.js"), "console.log(1);\n");
  if (withFallback)
    writeFileSync(join(distDir, "__spa-fallback.html"), "<!doctype html><title>test-fallback</title>\n");
  return { root, distDir };
}

function listen(server, host = "127.0.0.1") {
  return new Promise((resolve, reject) => {
    server.on("error", reject);
    server.listen(0, host, () => resolve(server.address().port));
  });
}

function httpFetch(port, path, options = {}) {
  return new Promise((resolve, reject) => {
    const { body, ...requestOptions } = options;
    const outgoing = httpRequest({ host: "127.0.0.1", port, path, method: "GET", ...requestOptions }, (res) => {
      const chunks = [];
      res.on("data", (chunk) => chunks.push(chunk));
      res.on("end", () =>
        resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks).toString("utf8") })
      );
      res.on("error", reject);
    });
    outgoing.on("error", reject);
    if (body !== undefined) outgoing.write(body);
    outgoing.end();
  });
}

/** Send raw bytes over TCP and resolve with everything received until close or idle. */
function rawExchange(port, bytes, { idleMs = 500 } = {}) {
  return new Promise((resolve) => {
    const socket = connect({ host: "127.0.0.1", port }, () => socket.write(bytes));
    const chunks = [];
    let timer = null;
    const done = () => {
      if (timer !== null) clearTimeout(timer);
      socket.destroy();
      resolve(Buffer.concat(chunks).toString("utf8"));
    };
    const arm = () => {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(done, idleMs);
    };
    socket.on("data", (chunk) => {
      chunks.push(chunk);
      arm();
    });
    socket.on("close", done);
    socket.on("error", () => done());
    arm();
  });
}

describe("parsePort", () => {
  it("accepts the ends of the range", () => {
    assert.equal(parsePort("1", "V"), 1);
    assert.equal(parsePort("65535", "V"), 65535);
    assert.equal(parsePort("3010", "V"), 3010);
  });
  it("rejects non-numeric, zero and out-of-range values naming the variable", () => {
    for (const raw of ["abc", "", "12a", "0", "65536", "-1", " 12"]) {
      assert.throws(() => parsePort(raw, "PARITY_NEW_PORT"), /PARITY_NEW_PORT must be a TCP port/);
    }
  });
});

describe("resolveApiOrigin", () => {
  it("defaults to the parity API port", () => {
    assert.equal(resolveApiOrigin({}), DEFAULT_API_ORIGIN);
    assert.equal(resolveApiOrigin({ PIDASH_API_ORIGIN: "  ", PARITY_API_URL: "" }), DEFAULT_API_ORIGIN);
  });
  it("prefers PIDASH_API_ORIGIN over PARITY_API_URL", () => {
    assert.equal(resolveApiOrigin({ PIDASH_API_ORIGIN: "http://a:1", PARITY_API_URL: "http://b:2" }), "http://a:1");
    assert.equal(resolveApiOrigin({ PIDASH_API_ORIGIN: " ", PARITY_API_URL: "http://b:2" }), "http://b:2");
  });
  it("strips any path, query or trailing slash", () => {
    assert.equal(resolveApiOrigin({ PARITY_API_URL: "http://h:9/base/?x=1" }), "http://h:9");
  });
  it("rejects non-URLs, non-http origins and embedded credentials", () => {
    assert.throws(() => resolveApiOrigin({ PIDASH_API_ORIGIN: "://bad" }), /not a URL/);
    assert.throws(() => resolveApiOrigin({ PIDASH_API_ORIGIN: "https://h:1" }), /must be plain http/);
    assert.throws(() => resolveApiOrigin({ PIDASH_API_ORIGIN: "http://user@h:1" }), /must be plain http/);
    assert.throws(() => resolveApiOrigin({ PIDASH_API_ORIGIN: "http://:pass@h:1" }), /must be plain http/);
  });
});

describe("resolveDistName", () => {
  it("serves web unless desktop is asked", () => {
    assert.equal(resolveDistName({}), "web");
    assert.equal(resolveDistName({ PARITY_NEW_DIST: " " }), "web");
    assert.equal(resolveDistName({ PARITY_NEW_DIST: "desktop" }), "desktop");
  });
  it("rejects anything else", () => {
    assert.throws(() => resolveDistName({ PARITY_NEW_DIST: "staging" }), /must be "web" or "desktop"/);
  });
});

describe("resolveConfig", () => {
  it("assembles the defaults from a scratch root", () => {
    const { root, distDir } = makeDist();
    assert.deepEqual(resolveConfig({}, root), {
      port: DEFAULT_NEW_PORT,
      distDir,
      apiOrigin: DEFAULT_API_ORIGIN,
      minioOrigin: null,
      liveOrigin: null,
      fallbackFile: join(distDir, "__spa-fallback.html"),
    });
  });
  it("falls back to the app shell when the build skipped prerender", () => {
    const { root, distDir } = makeDist({ withFallback: false });
    assert.equal(resolveConfig({}, root).fallbackFile, join(distDir, "index.html"));
  });
  it("honors every explicit setting", () => {
    const { root, distDir } = makeDist();
    const config = resolveConfig(
      {
        PARITY_NEW_PORT: "3020",
        PARITY_API_URL: "http://127.0.0.1:18057",
        PARITY_MINIO_PORT: "19057",
        PARITY_LIVE_PORT: "13157",
      },
      root
    );
    assert.deepEqual(config, {
      port: 3020,
      distDir,
      apiOrigin: "http://127.0.0.1:18057",
      minioOrigin: "http://localhost:19057",
      liveOrigin: "http://localhost:13157",
      fallbackFile: join(distDir, "__spa-fallback.html"),
    });
  });
  it("names the offending variable on bad ports", () => {
    const { root } = makeDist();
    assert.throws(() => resolveConfig({ PARITY_NEW_PORT: "bogus" }, root), /PARITY_NEW_PORT/);
    assert.throws(() => resolveConfig({ PARITY_MINIO_PORT: "bogus" }, root), /PARITY_MINIO_PORT/);
    assert.throws(() => resolveConfig({ PARITY_LIVE_PORT: "bogus" }, root), /PARITY_LIVE_PORT/);
  });
  it("refuses a root with no build", () => {
    const root = mkdtempSync(join(tmpdir(), "parity-new-empty-"));
    assert.throws(() => resolveConfig({}, root), /no build at .*; run `pnpm parity:build-new` first/);
  });
});

describe("proxyTargetFor", () => {
  const config = { apiOrigin: "http://api:1", minioOrigin: "http://minio:2", liveOrigin: "http://live:3" };
  it("routes each prefix family to its origin", () => {
    const cases = [
      ["/api", "http://api:1"],
      ["/api/issues", "http://api:1"],
      ["/auth", "http://api:1"],
      ["/auth/login/", "http://api:1"],
      ["/auth/device/x", "http://api:1"],
      ["/parity19-assets", "http://minio:2"],
      ["/parity19-assets/7f/logo.png", "http://minio:2"],
      ["/live", "http://live:3"],
      ["/live/collaboration", "http://live:3"],
    ];
    for (const [pathname, expected] of cases) assert.equal(proxyTargetFor(pathname, config), expected, pathname);
  });
  it("serves app routes and the device page from the build", () => {
    for (const pathname of [
      "/",
      "/ws/projects/p/modules",
      "/auth/device",
      "/auth/device/",
      "/apix",
      "/authx",
      "/livex",
    ]) {
      assert.equal(proxyTargetFor(pathname, config), null, pathname);
    }
  });
  it("falls back to static when minio or live are unset", () => {
    const bare = { apiOrigin: config.apiOrigin, minioOrigin: null, liveOrigin: null };
    assert.equal(proxyTargetFor("/parity19-assets/7f/logo.png", bare), null);
    assert.equal(proxyTargetFor("/live/collaboration", bare), null);
    assert.equal(proxyTargetFor("/api/issues", bare), "http://api:1");
  });
});

describe("upgradeTargetFor", () => {
  const config = { apiOrigin: "http://api:1", liveOrigin: "http://live:3" };
  it("tunnels api and live sockets only", () => {
    assert.equal(upgradeTargetFor("/api/notifications", config), "http://api:1");
    assert.equal(upgradeTargetFor("/api", config), "http://api:1");
    assert.equal(upgradeTargetFor("/live/collaboration", config), "http://live:3");
    assert.equal(upgradeTargetFor("/live", config), "http://live:3");
  });
  it("drops everything else, including live when unset", () => {
    assert.equal(upgradeTargetFor("/live/collaboration", { apiOrigin: "http://api:1", liveOrigin: null }), null);
    for (const pathname of ["/", "/apix", "/livex", "/auth/device", "/parity19-assets/x"]) {
      assert.equal(upgradeTargetFor(pathname, config), null, pathname);
    }
  });
});

describe("rewriteRedirect", () => {
  it("points loopback redirects back at the serving host", () => {
    assert.equal(
      rewriteRedirect("http://localhost:18057/auth/x?y=1", "127.0.0.1:3010"),
      "http://127.0.0.1:3010/auth/x?y=1"
    );
    assert.equal(rewriteRedirect("http://127.0.0.1:9/a", "h:1"), "http://h:1/a");
    assert.equal(rewriteRedirect("http://[::1]:9/a", "h:1"), "http://h:1/a");
  });
  it("leaves external, relative and invalid locations alone", () => {
    assert.equal(rewriteRedirect("https://provider.example/o", "h:1"), "https://provider.example/o");
    assert.equal(rewriteRedirect("/relative/path", "h:1"), "/relative/path");
    assert.equal(rewriteRedirect("://bad", "h:1"), "://bad");
    assert.equal(rewriteRedirect("", "h:1"), "");
  });
  it("leaves already-correct and hostless requests alone", () => {
    assert.equal(rewriteRedirect("http://h:1/a", "h:1"), "http://h:1/a");
    assert.equal(rewriteRedirect("http://localhost:1/a", undefined), "http://localhost:1/a");
    assert.equal(rewriteRedirect("http://localhost:1/a", ""), "http://localhost:1/a");
    assert.equal(rewriteRedirect("http://localhost:1/a", 42), "http://localhost:1/a");
  });
});

describe("contentTypeFor", () => {
  it("maps every served suffix and lowercases", () => {
    const cases = [
      ["app.js", "text/javascript; charset=utf-8"],
      ["app.CSS", "text/css; charset=utf-8"],
      ["index.html", "text/html; charset=utf-8"],
      ["icon.ico", "image/x-icon"],
      ["data.json", "application/json; charset=utf-8"],
      ["app.js.map", "application/json; charset=utf-8"],
      ["logo.png", "image/png"],
      ["photo.jpg", "image/jpeg"],
      ["icon.svg", "image/svg+xml"],
      ["robots.txt", "text/plain; charset=utf-8"],
      ["manifest.webmanifest", "application/manifest+json"],
      ["font.woff2", "font/woff2"],
      ["sitemap.xml", "application/xml"],
    ];
    for (const [name, expected] of cases) assert.equal(contentTypeFor(name), expected, name);
  });
  it("treats unknown and missing suffixes as opaque bytes", () => {
    assert.equal(contentTypeFor("archive.zip"), "application/octet-stream");
    assert.equal(contentTypeFor("LICENSE"), "application/octet-stream");
  });
});

describe("cacheControlFor", () => {
  it("freezes hashed assets and revalidates everything else", () => {
    assert.equal(cacheControlFor("/assets/app-abc.js"), "public, max-age=31536000, immutable");
    assert.equal(cacheControlFor("/assets"), "public, max-age=31536000, immutable");
    assert.equal(cacheControlFor("/"), "no-store");
    assert.equal(cacheControlFor("/assetsx/app.js"), "no-store");
    assert.equal(cacheControlFor("/index.html"), "no-store");
  });
});

describe("staticFileFor", () => {
  it("tries the directory index first, then the exact file, then the fallback", () => {
    const { root, distDir } = makeDist();
    const config = resolveConfig({}, root);
    assert.equal(staticFileFor(config, "/home"), join(distDir, "home", "index.html"));
    assert.equal(staticFileFor(config, "/home/"), join(distDir, "home", "index.html"));
    assert.equal(staticFileFor(config, "/"), join(distDir, "index.html"));
    assert.equal(staticFileFor(config, "/assets/app-abc123.js"), join(distDir, "assets", "app-abc123.js"));
    assert.equal(staticFileFor(config, "/ws/projects/p/modules"), join(distDir, "__spa-fallback.html"));
    assert.equal(staticFileFor(config, "/subdir"), join(distDir, "__spa-fallback.html"));
    assert.equal(staticFileFor(config, "/subdir/"), join(distDir, "__spa-fallback.html"));
    // Suffixed misses fall back too, exactly like the production nginx rule.
    assert.equal(staticFileFor(config, "/missing.js"), join(distDir, "__spa-fallback.html"));
    // A directory shaped like an index, and the directory itself, both miss.
    assert.equal(staticFileFor(config, "/xdir"), join(distDir, "__spa-fallback.html"));
  });
  it("clamps parent segments at the build root, never escaping it", () => {
    const { root, distDir } = makeDist();
    const config = resolveConfig({}, root);
    assert.equal(staticFileFor(config, "/../../secret"), join(distDir, "__spa-fallback.html"));
    assert.equal(staticFileFor(config, "/ws/../../secret"), join(distDir, "__spa-fallback.html"));
  });
});

describe("static serving", () => {
  it("serves the shell, hashed assets and the SPA fallback with their policies", async () => {
    const { root } = makeDist();
    const server = track(createProdServer(resolveConfig({}, root)));
    const port = await listen(server);
    const shell = await httpFetch(port, "/");
    assert.equal(shell.status, 200);
    assert.match(shell.body, /test-index/);
    assert.equal(shell.headers["content-type"], "text/html; charset=utf-8");
    assert.equal(shell.headers["cache-control"], "no-store");
    const asset = await httpFetch(port, "/assets/app-abc123.js");
    assert.equal(asset.status, 200);
    assert.equal(asset.headers["content-type"], "text/javascript; charset=utf-8");
    assert.equal(asset.headers["cache-control"], "public, max-age=31536000, immutable");
    const home = await httpFetch(port, "/home");
    assert.equal(home.status, 200);
    assert.match(home.body, /test-home/);
    const deep = await httpFetch(port, "/ws/projects/p/modules");
    assert.equal(deep.status, 200);
    assert.match(deep.body, /test-fallback/);
    const device = await httpFetch(port, "/auth/device");
    assert.equal(device.status, 200);
    assert.match(device.body, /test-fallback/);
    // Suffixed misses fall back too, exactly like the production nginx rule.
    const miss = await httpFetch(port, "/missing.js");
    assert.equal(miss.status, 200);
    assert.match(miss.body, /test-fallback/);
  });
  it("answers HEAD without a body, never escapes the build, 405s writes", async () => {
    const { root } = makeDist();
    writeFileSync(join(root, "outside.txt"), "outside-marker\n");
    const server = track(createProdServer(resolveConfig({}, root)));
    const port = await listen(server);
    const head = await httpFetch(port, "/", { method: "HEAD" });
    assert.equal(head.status, 200);
    assert.equal(head.body, "");
    assert.equal(head.headers["content-length"], String("<!doctype html><title>test-index</title>\n".length));
    // Encoded separators stay a weird file name (the fallback); literal
    // dot segments are normalized away by URL parsing. Either way the
    // file outside the build is unreachable.
    for (const path of ["/..%2f..%2foutside.txt", "/../outside.txt", "/ws/../../outside.txt"]) {
      const response = await httpFetch(port, path);
      assert.equal(response.status, 200, path);
      assert.doesNotMatch(response.body, /outside-marker/, path);
    }
    assert.equal((await httpFetch(port, "/", { method: "POST" })).status, 405);
  });
  it("rejects a malformed request target with 400", async () => {
    const { root } = makeDist();
    const server = track(createProdServer(resolveConfig({}, root)));
    const port = await listen(server);
    // Absolute-form garbage: llhttp accepts the target, the URL parser does not.
    const raw = await rawExchange(port, "GET http://[bad HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    assert.match(raw, /^HTTP\/1\.1 400/);
  });
  it("destroys the socket on parser garbage", async () => {
    const { root } = makeDist();
    const server = track(createProdServer(resolveConfig({}, root)));
    const port = await listen(server);
    const raw = await rawExchange(port, "NOTHTTP\r\n\r\n");
    assert.equal(raw, "");
  });
});

describe("api proxying", () => {
  it("forwards method, path, body and host, and rewrites loopback redirects", async () => {
    const seen = [];
    const backend = track(
      createHttpServer((req, res) => {
        const chunks = [];
        req.on("data", (chunk) => chunks.push(chunk));
        req.on("end", () => {
          seen.push({
            method: req.method,
            url: req.url,
            host: req.headers.host,
            body: Buffer.concat(chunks).toString(),
          });
          if (req.url === "/api/loop") {
            res.writeHead(302, { location: `http://localhost:${backend.address().port}/auth/x` });
            res.end();
            return;
          }
          if (req.url === "/api/away") {
            res.writeHead(302, { location: "https://provider.example/o" });
            res.end();
            return;
          }
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({ ok: true }));
        });
      })
    );
    const backendPort = await listen(backend);
    const { root } = makeDist();
    const server = track(
      createProdServer(resolveConfig({ PIDASH_API_ORIGIN: `http://127.0.0.1:${backendPort}` }, root))
    );
    const port = await listen(server);
    const get = await httpFetch(port, "/api/issues?x=1");
    assert.equal(get.status, 200);
    assert.deepEqual(JSON.parse(get.body), { ok: true });
    const post = await httpFetch(port, "/auth/login", { method: "POST", body: "email=a@b.c" });
    assert.equal(post.status, 200);
    assert.deepEqual(
      seen.map((entry) => [entry.method, entry.url, entry.body]),
      [
        ["GET", "/api/issues?x=1", ""],
        ["POST", "/auth/login", "email=a@b.c"],
      ]
    );
    // changeOrigin false: the backend sees this server's Host, not its own.
    assert.deepEqual(
      seen.map((entry) => entry.host),
      [`127.0.0.1:${port}`, `127.0.0.1:${port}`]
    );
    const loop = await httpFetch(port, "/api/loop");
    assert.equal(loop.status, 302);
    assert.equal(loop.headers["location"], `http://127.0.0.1:${port}/auth/x`);
    const away = await httpFetch(port, "/api/away");
    assert.equal(away.headers["location"], "https://provider.example/o");
  });
  it("answers 502 and logs when the backend is down", async () => {
    const { root } = makeDist();
    const closed = await freePort();
    const server = track(createProdServer(resolveConfig({ PIDASH_API_ORIGIN: `http://127.0.0.1:${closed}` }, root)));
    const port = await listen(server);
    const errors = [];
    const original = console.error;
    console.error = (...args) => errors.push(args.join(" "));
    try {
      const response = await httpFetch(port, "/api/issues");
      assert.equal(response.status, 502);
    } finally {
      console.error = original;
    }
    assert.equal(errors.length, 1);
    assert.match(errors[0], /proxy GET \/api\/issues .* failed: Error: connect ECONNREFUSED/);
  });
  it("survives a backend that dies mid-response", async () => {
    const backend = track(
      createHttpServer((_req, res) => {
        res.writeHead(200, { "content-type": "text/plain" });
        res.write("partial");
        res.socket.destroy();
      })
    );
    const backendPort = await listen(backend);
    const { root } = makeDist();
    const server = track(
      createProdServer(resolveConfig({ PIDASH_API_ORIGIN: `http://127.0.0.1:${backendPort}` }, root))
    );
    const port = await listen(server);
    const outcome = await httpFetch(port, "/api/stream").then(
      (response) => `ended:${response.body}`,
      (error) => `errored:${error.code ?? error.message}`
    );
    assert.match(outcome, /^(ended|errored)/);
  });
});

describe("websocket tunneling", () => {
  const HANDSHAKE =
    "GET /api/ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";

  /** A TCP target that completes one handshake, then echoes every later byte. */
  function echoTarget(onHandshake) {
    const received = [];
    const target = track(
      createTcpServer((socket) => {
        let handshook = false;
        socket.on("data", (chunk) => {
          if (!handshook) {
            handshook = true;
            onHandshake(chunk.toString("utf8"));
            socket.write("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n");
            return;
          }
          received.push(chunk.toString("utf8"));
          socket.write(chunk);
        });
      })
    );
    return { target, received };
  }

  it("tunnels an api socket and echoes through it", async () => {
    let handshake = "";
    const { target, received } = echoTarget((text) => (handshake = text));
    const targetPort = await listen(target);
    const { root } = makeDist();
    const server = track(
      createProdServer(resolveConfig({ PIDASH_API_ORIGIN: `http://127.0.0.1:${targetPort}` }, root))
    );
    const port = await listen(server);
    const socket = connect({ host: "127.0.0.1", port });
    const chunks = [];
    await new Promise((resolve, reject) => {
      socket.on("connect", () => socket.write(HANDSHAKE));
      socket.on("data", (chunk) => {
        chunks.push(chunk.toString("utf8"));
        const text = chunks.join("");
        if (text.includes("101")) {
          socket.write("PING");
        }
        if (text.includes("PING")) resolve();
      });
      socket.on("error", reject);
      setTimeout(() => reject(new Error("tunnel timed out")), 5000);
    });
    socket.destroy();
    assert.match(handshake, /^GET \/api\/ws HTTP\/1\.1\r\n/);
    assert.match(handshake, /Upgrade: websocket/);
    assert.deepEqual(received, ["PING"]);
  });
  it("forwards bytes pipelined behind the handshake", async () => {
    let handshake = "";
    const { target } = echoTarget((text) => (handshake = text));
    const targetPort = await listen(target);
    const { root } = makeDist();
    const server = track(
      createProdServer(resolveConfig({ PIDASH_API_ORIGIN: `http://127.0.0.1:${targetPort}` }, root))
    );
    const port = await listen(server);
    const raw = await rawExchange(port, `${HANDSHAKE}HEAD-BYTES`, { idleMs: 800 });
    assert.match(raw, /101 Switching Protocols/);
    assert.match(handshake, /HEAD-BYTES$/);
  });
  it("drops upgrades with no target and malformed targets", async () => {
    const { root } = makeDist();
    const server = track(createProdServer(resolveConfig({}, root)));
    const port = await listen(server);
    const live = rawExchange(
      port,
      "GET /live/collaboration HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n"
    );
    const other = rawExchange(
      port,
      "GET /other HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n"
    );
    const malformed = rawExchange(
      port,
      "GET http://[bad HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n"
    );
    assert.deepEqual(await Promise.all([live, other, malformed]), ["", "", ""]);
  });
});

describe("tunnelUpgrade", () => {
  it("forwards a stub handshake without headers or head", async () => {
    let handshake = "";
    const target = track(
      createTcpServer((socket) => {
        socket.on("data", (chunk) => {
          handshake += chunk.toString("utf8");
          socket.destroy();
        });
      })
    );
    const targetPort = await listen(target);
    const browserish = track(createTcpServer());
    const browserishPort = await listen(browserish);
    const socket = connect({ host: "127.0.0.1", port: browserishPort });
    await new Promise((resolve) => socket.on("connect", resolve));
    tunnelUpgrade(
      { method: "GET", url: "/api/ws", httpVersion: "1.1" },
      socket,
      undefined,
      `http://127.0.0.1:${targetPort}`
    );
    await new Promise((resolve) => socket.on("close", resolve));
    assert.match(handshake, /^GET \/api\/ws HTTP\/1\.1\r\n\r\n/);
  });
  it("destroys the socket when the upstream refuses", async () => {
    const closed = await freePort();
    const browserish = track(createTcpServer());
    const browserishPort = await listen(browserish);
    const socket = connect({ host: "127.0.0.1", port: browserishPort });
    await new Promise((resolve) => socket.on("connect", resolve));
    tunnelUpgrade(
      { method: "GET", url: "/api/ws", httpVersion: "1.1", rawHeaders: [] },
      socket,
      Buffer.alloc(0),
      `http://127.0.0.1:${closed}`
    );
    await new Promise((resolve) => socket.on("close", resolve));
    assert.equal(socket.destroyed, true);
  });
  it("destroys the upstream when the socket errors", async () => {
    let upstreamClosed = false;
    const target = track(
      createTcpServer((socket) => {
        socket.on("data", () => socket.write("HTTP/1.1 101 Switching Protocols\r\n\r\n"));
        socket.on("close", () => (upstreamClosed = true));
      })
    );
    const targetPort = await listen(target);
    const browserish = track(createTcpServer());
    const browserishPort = await listen(browserish);
    const socket = connect({ host: "127.0.0.1", port: browserishPort });
    await new Promise((resolve) => socket.on("connect", resolve));
    socket.on("error", () => {});
    tunnelUpgrade(
      { method: "GET", url: "/api/ws", httpVersion: "1.1", rawHeaders: ["Host", "x"] },
      socket,
      Buffer.alloc(0),
      `http://127.0.0.1:${targetPort}`
    );
    await new Promise((resolve) => setTimeout(resolve, 200));
    socket.emit("error", new Error("boom"));
    await new Promise((resolve) => setTimeout(resolve, 200));
    assert.equal(upstreamClosed, true);
    socket.destroy();
  });
});

function freePort() {
  return new Promise((resolve, reject) => {
    const probe = createTcpServer();
    probe.on("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const port = probe.address().port;
      probe.close(() => resolve(port));
    });
  });
}

/** Run the script as a subprocess with piped output. */
function runScript(env) {
  const child = spawn(process.execPath, [script], {
    env: { ...process.env, ...env },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => (stdout += chunk));
  child.stderr.on("data", (chunk) => (stderr += chunk));
  return {
    child,
    waitForOutput: (pattern, timeoutMs = 10000) =>
      new Promise((resolve, reject) => {
        const timer = setTimeout(
          () => reject(new Error(`timed out waiting for ${pattern}; stderr: ${stderr}`)),
          timeoutMs
        );
        const poll = () => {
          if (pattern.test(stdout)) {
            clearTimeout(timer);
            resolve(stdout);
            return;
          }
          if (child.exitCode !== null) {
            clearTimeout(timer);
            reject(new Error(`exited ${child.exitCode} waiting for ${pattern}; stderr: ${stderr}`));
            return;
          }
          setTimeout(poll, 50);
        };
        poll();
      }),
    waitForExit: (timeoutMs = 10000) =>
      new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          child.kill("SIGKILL");
          reject(new Error(`timed out waiting for exit; stdout: ${stdout}; stderr: ${stderr}`));
        }, timeoutMs);
        child.on("close", (code) => {
          clearTimeout(timer);
          resolve({ code, stdout, stderr });
        });
      }),
  };
}

describe("entry point", () => {
  it("refuses invalid env with exit 2", async () => {
    const run = runScript({ PARITY_NEW_PORT: "bogus" });
    const { code, stderr } = await run.waitForExit();
    assert.equal(code, 2);
    assert.match(stderr, /PARITY_NEW_PORT must be a TCP port/);
  });
  it("serves a scratch root and logs its targets", async () => {
    const { root } = makeDist();
    const port = await freePort();
    const run = runScript({
      PARITY_NEW_PORT: String(port),
      PARITY_NEW_ROOT: root,
      PIDASH_API_ORIGIN: "http://127.0.0.1:18057",
      PARITY_MINIO_PORT: "19057",
      PARITY_LIVE_PORT: "13157",
    });
    try {
      const boot = await run.waitForOutput(/\[parity:new\] serving /);
      assert.match(boot, new RegExp(`http://localhost:${port}`));
      assert.match(boot, /api http:\/\/127\.0\.0\.1:18057/);
      assert.match(boot, /assets http:\/\/localhost:19057/);
      assert.match(boot, /live http:\/\/localhost:13157/);
      const response = await httpFetch(port, "/");
      assert.equal(response.status, 200);
      assert.match(response.body, /test-index/);
    } finally {
      run.child.kill("SIGKILL");
      await run.waitForExit();
    }
  });
  it("exits 1 when the port is taken", async () => {
    const { root } = makeDist();
    const squatter = track(createHttpServer((_req, res) => res.end()));
    const port = await listen(squatter);
    const run = runScript({ PARITY_NEW_PORT: String(port), PARITY_NEW_ROOT: root });
    const { code, stderr } = await run.waitForExit();
    assert.equal(code, 1);
    assert.match(stderr, new RegExp(`cannot serve port ${port}`));
  });
  it("imports without serving", async () => {
    const child = spawn(
      process.execPath,
      ["-e", `import(${JSON.stringify(script)}).then(() => console.log("imported"))`],
      {
        stdio: ["ignore", "pipe", "pipe"],
      }
    );
    let stdout = "";
    child.stdout.on("data", (chunk) => (stdout += chunk));
    const code = await new Promise((resolve) => child.on("close", resolve));
    assert.equal(code, 0);
    assert.match(stdout, /imported/);
  });
});
