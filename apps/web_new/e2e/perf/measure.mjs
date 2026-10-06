// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Baseline measurement harness (NEWFRONT-21). One method for both apps:
// sign in once through the served origin's own /auth proxy, then load the
// issue list cold (fresh profile) and warm (primed HTTP cache) and record
// the JS fetched plus the time to painted rows. Desktop launches measure
// the desktop bundle the same way (the Tauri shell loads these same files
// from disk; native window overhead is outside frontend code).
//
//   node measure.mjs --app-url http://localhost:3031 --kind web-new \
//     --email user@example.com --password secret --workspace ws \
//     --project <uuid> --issue-name "Seeded issue" --out baseline.json
import { gzipSync } from "node:zlib";
import { writeFile } from "node:fs/promises";
import { chromium, request } from "@playwright/test";

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!flag.startsWith("--") || value === undefined) throw new Error(`bad args near ${flag}`);
    out[flag.slice(2)] = value;
  }
  for (const key of ["app-url", "kind", "email", "password", "workspace", "project", "issue-name", "out"]) {
    if (!out[key]) throw new Error(`missing --${key}`);
  }
  if (out.kind !== "web-new" && out.kind !== "web-old") throw new Error("--kind must be web-new or web-old");
  return out;
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.floor(sorted.length / 2)];
}

async function signInState(appUrl, email, password) {
  const api = await request.newContext({ baseURL: appUrl });
  try {
    const tokenRes = await api.get("/auth/get-csrf-token/");
    if (!tokenRes.ok()) throw new Error(`CSRF fetch failed: HTTP ${tokenRes.status()}`);
    const token = (await tokenRes.json())?.csrf_token;
    if (typeof token !== "string" || token === "") throw new Error("CSRF response carried no token.");
    const login = await api.post("/auth/sign-in/", {
      form: { email, password, csrfmiddlewaretoken: token },
    });
    if (login.url().includes("error_code=")) throw new Error(`sign-in rejected: ${login.url()}`);
    const me = await api.get("/api/users/me/");
    if (!me.ok()) throw new Error(`session check failed: HTTP ${me.status()}`);
    return await api.storageState();
  } finally {
    await api.dispose();
  }
}

async function rowsVisible(page, kind, issueName) {
  if (kind === "web-new") {
    const region = page.getByRole("region", { name: "Issues" });
    await region.getByRole("article").first().waitFor({ timeout: 30_000 });
    await region.getByText(issueName).first().waitFor({ timeout: 10_000 });
  } else {
    await page.getByRole("main").getByText(issueName).first().waitFor({ timeout: 60_000 });
  }
}

async function measuredLoad(browser, state, url, kind, issueName) {
  const context = await browser.newContext({ storageState: state });
  try {
    const page = await context.newPage();
    const scripts = [];
    page.on("response", (response) => {
      const req = response.request();
      if (req.resourceType() === "script" || /\.js(\?|$)/.test(req.url())) {
        scripts.push(response);
      }
    });
    const started = Date.now();
    await page.goto(url);
    await rowsVisible(page, kind, issueName);
    const wallMs = Date.now() - started;
    const navigation = await page.evaluate(() => {
      const entry = performance.getEntriesByType("navigation")[0];
      return entry
        ? {
            responseStartMs: Math.round(entry.responseStart),
            domContentLoadedMs: Math.round(entry.domContentLoadedEventEnd),
            loadMs: Math.round(entry.loadEventEnd),
            transferBytes: entry.transferSize,
          }
        : null;
    });
    let raw = 0;
    let gzip = 0;
    for (const response of scripts) {
      try {
        const body = await response.body();
        raw += body.length;
        gzip += gzipSync(body).length;
      } catch {
        // A failed or evicted entry contributes nothing; the count stands.
      }
    }
    return { wallMs, jsFiles: scripts.length, jsRawBytes: raw, jsGzipBytes: gzip, navigation };
  } finally {
    await context.close();
  }
}

async function measuredWarm(browser, state, url, kind, issueName) {
  const context = await browser.newContext({ storageState: state });
  try {
    const page = await context.newPage();
    await page.goto(url);
    await rowsVisible(page, kind, issueName);
    const runs = [];
    for (let i = 0; i < 3; i++) {
      const started = Date.now();
      await page.goto(url);
      await rowsVisible(page, kind, issueName);
      runs.push(Date.now() - started);
    }
    return runs;
  } finally {
    await context.close();
  }
}

const args = parseArgs(process.argv.slice(2));
const appUrl = args["app-url"].replace(/\/+$/, "");
const issuesUrl = `${appUrl}/${args.workspace}/projects/${args.project}/issues`;

const browser = await chromium.launch();
try {
  console.log(`[perf] signing in ${args.email} through ${appUrl}`);
  const state = await signInState(appUrl, args.email, args.password);
  console.log("[perf] session ok; running 3 cold loads");
  const cold = [];
  for (let i = 0; i < 3; i++) {
    cold.push(await measuredLoad(browser, state, issuesUrl, args.kind, args["issue-name"]));
    console.log(
      `[perf] cold ${i + 1}: ${cold[i].wallMs} ms, ${cold[i].jsFiles} js files, ` +
        `${(cold[i].jsRawBytes / 1024).toFixed(0)} KB raw / ${(cold[i].jsGzipBytes / 1024).toFixed(0)} KB gzip`
    );
  }
  console.log("[perf] running warm loads");
  const warm = await measuredWarm(browser, state, issuesUrl, args.kind, args["issue-name"]);
  console.log(`[perf] warm: ${warm.join(" / ")} ms`);
  const report = {
    appUrl,
    kind: args.kind,
    at: new Date().toISOString(),
    cold: {
      wallMs: median(cold.map((c) => c.wallMs)),
      jsFiles: median(cold.map((c) => c.jsFiles)),
      jsRawBytes: median(cold.map((c) => c.jsRawBytes)),
      jsGzipBytes: median(cold.map((c) => c.jsGzipBytes)),
      domContentLoadedMs: median(cold.map((c) => c.navigation?.domContentLoadedMs ?? 0)),
      loadMs: median(cold.map((c) => c.navigation?.loadMs ?? 0)),
    },
    warmMs: median(warm),
    warmRunsMs: warm,
    coldRuns: cold,
  };
  await writeFile(args.out, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`[perf] wrote ${args.out}`);
} finally {
  await browser.close();
}
