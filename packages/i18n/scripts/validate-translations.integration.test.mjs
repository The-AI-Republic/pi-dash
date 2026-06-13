// Integration coverage for the validate → translate report pipeline. A local
// HTTP server stands in for the OpenAI-compatible endpoint so the scripts'
// real HTTP/parse/report code runs without a network or repo mutation: validate
// writes only to a temp report path, and the translate run below is steered to a
// no-op (a key that doesn't exist in the locale) so it never edits locale files.

import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { spawn } from "node:child_process";
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const scriptsDir = import.meta.dirname;
const validateScript = path.join(scriptsDir, "validate-translations.mjs");
const translateScript = path.join(scriptsDir, "translate-missing.mjs");

let server;
let baseUrl;
let lastRequestBody;

function startServer(responder) {
  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => {
      let body = "";
      req.on("data", (chunk) => {
        body += chunk;
      });
      req.on("end", () => {
        lastRequestBody = JSON.parse(body);
        const content = responder(lastRequestBody);
        res.writeHead(200, { "Content-Type": "application/json" });
        res.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify(content) } }] }));
      });
    });
    srv.listen(0, "127.0.0.1", () => {
      const { port } = srv.address();
      resolve({ srv, url: `http://127.0.0.1:${port}` });
    });
  });
}

// IMPORTANT: use async spawn, not spawnSync. The mock HTTP server runs in THIS
// process's event loop; spawnSync would block it and the subprocess's request
// would hang until its timeout. Async spawn keeps the parent loop serving HTTP.
function runScript(scriptPath, extraArgs) {
  return new Promise((resolve) => {
    const child = spawn(
      "node",
      [
        scriptPath,
        "--provider",
        "openai",
        "--api-key",
        "test-key",
        "--model",
        "test-model",
        "--base-url",
        baseUrl,
        "--retry-count",
        "0",
        "--request-timeout-ms",
        "15000",
        ...extraArgs,
      ],
      { encoding: "utf8" }
    );
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("close", (code) => resolve({ status: code, stdout, stderr }));
  });
}

describe("validate-translations integration", () => {
  beforeAll(async () => {
    // The reviewer flags the first item with a plain-text suggestion (safe when
    // the source has no ICU args) and returns an empty array for any later batch.
    const started = await startServer((reqBody) => {
      const items = JSON.parse(reqBody.messages[1].content).items;
      const first = items[0];
      return [{ id: first.id, reason: "test: wrong meaning", suggestion: "CORRECTED" }];
    });
    server = started.srv;
    baseUrl = started.url;
  });

  afterAll(() => {
    server?.close();
  });

  it("reviews existing translations and writes a JSON report a reviewer flagged", async () => {
    const out = path.join(os.tmpdir(), `i18n-report-${process.pid}.json`);
    try {
      const result = await runScript(validateScript, [
        "--languages",
        "fr",
        "--limit",
        "3",
        "--batch-size",
        "3",
        "--out",
        out,
      ]);
      expect(result.status, result.stderr).toBe(0);
      expect(fs.existsSync(out)).toBe(true);

      const report = JSON.parse(fs.readFileSync(out, "utf8"));
      expect(report.generatedBy).toBe("i18n:validate");
      expect(report.model).toBe("test-model");
      expect(Array.isArray(report.issues)).toBe(true);
      expect(report.issueCount).toBe(report.issues.length);
      expect(report.issueCount).toBeGreaterThanOrEqual(1);

      const issue = report.issues[0];
      expect(issue.locale).toBe("fr");
      expect(typeof issue.key).toBe("string");
      expect(issue.reason).toBe("test: wrong meaning");
    } finally {
      fs.rmSync(out, { force: true });
    }
  });

  it("translate consumes a report and no-ops safely when the flagged key is absent", async () => {
    const out = path.join(os.tmpdir(), `i18n-report-noop-${process.pid}.json`);
    const report = {
      generatedBy: "i18n:validate",
      issues: [
        {
          locale: "fr",
          key: "__nonexistent_key_for_test__",
          current: "whatever",
          reason: "x",
          suggestion: "y",
        },
      ],
    };
    fs.writeFileSync(out, JSON.stringify(report));
    try {
      const result = await runScript(translateScript, [
        "--languages",
        "fr",
        "--skip-empty",
        "--skip-readme",
        "--validation-report",
        out,
      ]);
      expect(result.status, result.stderr).toBe(0);
      expect(result.stdout).toContain("no applicable validation entries");
    } finally {
      fs.rmSync(out, { force: true });
    }
  });

  it("translate rejects a malformed validation report", async () => {
    const out = path.join(os.tmpdir(), `i18n-report-bad-${process.pid}.json`);
    fs.writeFileSync(out, "not json{");
    try {
      const result = await runScript(translateScript, [
        "--languages",
        "fr",
        "--skip-empty",
        "--skip-readme",
        "--validation-report",
        out,
      ]);
      expect(result.status).not.toBe(0);
      expect(result.stderr + result.stdout).toMatch(/not valid JSON/);
    } finally {
      fs.rmSync(out, { force: true });
    }
  });
});
