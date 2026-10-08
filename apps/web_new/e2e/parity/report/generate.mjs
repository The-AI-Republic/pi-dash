// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Parity report generator (NEWFRONT-19). Joins the feature inventory
// (one row per capability, with its Status column) with the latest
// Playwright JSON results (tagged scenario titles per project) into one
// markdown report: per area, inventory rows by oracle/new green. Runs in CI
// without any stack; the results file is optional, so the report stays
// useful before the first scenario lands.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(join(here, "..", "..", "..", "..", ".."));

function arg(name, fallback) {
  const argv = process.argv;
  const joined = argv.find((entry) => entry.startsWith(`${name}=`));
  if (joined !== undefined) return joined.slice(name.length + 1);
  const bare = argv.indexOf(name);
  if (bare !== -1 && bare + 1 < argv.length) return argv[bare + 1] ?? fallback;
  return fallback;
}

const inventoryDir = resolve(repoRoot, arg("--inventory", ".ai_design/new_frontend_implementation/parity/inventory"));
function hasFlag(name) {
  return process.argv.some((entry) => entry === name || entry.startsWith(`${name}=`));
}

const resultsFile = hasFlag("--results")
  ? resolve(repoRoot, arg("--results", "apps/web_new/test-results/parity-results.json"))
  : null;
const outFile = hasFlag("--out") ? resolve(repoRoot, arg("--out", "")) : null;

function areaOf(filename) {
  return filename.replace(/\.md$/, "");
}

function parseInventory() {
  const rows = [];
  for (const filename of readdirSync(inventoryDir).sort()) {
    if (!filename.endsWith(".md")) continue;
    const area = areaOf(filename);
    const lines = readFileSync(join(inventoryDir, filename), "utf8").split("\n");
    for (const line of lines) {
      if (!line.startsWith("|")) continue;
      const cells = line.split("|").map((cell) => cell.trim());
      const id = cells[1] ?? "";
      if (!/^[A-Z]+-\d+$/.test(id)) continue;
      const status = (cells[cells.length - 2] ?? "").toLowerCase();
      rows.push({
        area,
        id,
        oracle: status.includes("oracle green") || status.includes("new green"),
        fresh: status.includes("new green"),
      });
    }
  }
  return rows;
}

function collectSpecs(node, acc) {
  if (node === null || typeof node !== "object") return;
  if (typeof node.title === "string" && Array.isArray(node.tests)) {
    const ids = (/^\[([A-Za-z]+-\d+(?:\s*,\s*[A-Za-z]+-\d+)*)\]/.exec(node.title)?.[1] ?? "")
      .split(",")
      .map((part) => part.trim().toUpperCase())
      .filter((part) => part.length > 0);
    for (const test of node.tests) {
      const project = test.projectName === "new" ? "new" : "oracle";
      const ok = (test.results ?? []).every((result) => result.status === "passed" || result.status === "skipped");
      for (const id of ids) acc.push({ id, project, ok });
    }
  }
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) for (const entry of value) collectSpecs(entry, acc);
    else if (typeof value === "object") collectSpecs(value, acc);
  }
}

function parseResults() {
  if (resultsFile === null) return [];
  try {
    const payload = JSON.parse(readFileSync(resultsFile, "utf8"));
    const acc = [];
    collectSpecs(payload, acc);
    return acc;
  } catch {
    return [];
  }
}

const inventory = parseInventory();
const runs = parseResults();
const runById = new Map();
for (const run of runs) {
  const key = `${run.id}|${run.project}`;
  const prev = runById.get(key);
  runById.set(key, prev === undefined ? run.ok : prev && run.ok);
}

const areas = [...new Set(inventory.map((row) => row.area))].sort();
let oracleTotal = 0;
let freshTotal = 0;
const out = [
  "# Parity report",
  "",
  `Generated ${new Date().toISOString()} from the feature inventory${resultsFile === null ? "" : " and the latest Playwright results"}.`,
  "",
];

for (const area of areas) {
  const areaRows = inventory.filter((row) => row.area === area);
  const oracle = areaRows.filter((row) => row.oracle).length;
  const fresh = areaRows.filter((row) => row.fresh).length;
  oracleTotal += oracle;
  freshTotal += fresh;
  out.push(`## ${area} — ${oracle}/${areaRows.length} oracle green, ${fresh}/${areaRows.length} new green`, "");
  const lit = areaRows.filter((row) => row.oracle || runById.has(`${row.id}|oracle`) || runById.has(`${row.id}|new`));
  if (lit.length === 0) {
    out.push("No row green yet.", "");
    continue;
  }
  out.push("| Row | Oracle | New | Last oracle run | Last new run |", "| --- | --- | --- | --- | --- |");
  for (const row of lit) {
    const lastOracle = runById.get(`${row.id}|oracle`);
    const lastNew = runById.get(`${row.id}|new`);
    const mark = (value) => (value === undefined ? "—" : value ? "pass" : "fail");
    out.push(
      `| ${row.id} | ${row.oracle ? "green" : "—"} | ${row.fresh ? "green" : "—"} | ${mark(lastOracle)} | ${mark(lastNew)} |`
    );
  }
  out.push("");
}

out.push(`Total: ${oracleTotal}/${inventory.length} oracle green, ${freshTotal}/${inventory.length} new green.`, "");
const report = out.join("\n");

if (outFile === null) process.stdout.write(report);
else writeFileSync(outFile, report);
