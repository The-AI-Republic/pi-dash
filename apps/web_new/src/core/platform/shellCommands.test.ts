// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Guard against the desktop platform and the Tauri shell drifting apart
// (PIDESKAPP-222): every native command this platform invokes must be one
// the shell registers in `generate_handler!`, or the call rejects with
// "command … not found" at run time while every mocked test stays green.
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../../../../..");

/** Command names the platform passes to an invoke call as a string literal. */
export function invokedCommands(source: string): string[] {
  const names = new Set<string>();
  for (const match of source.matchAll(/\binvoke[A-Za-z]*\s*(?:<[^>()]*>)?\s*\(\s*"([^"]+)"/g)) {
    const name = match[1];
    // Plugin commands (`plugin:window|set_title`) come from Tauri plugins,
    // not the app's own handler list.
    if (name !== undefined && !name.startsWith("plugin:")) names.add(name);
  }
  return [...names].sort();
}

/** Command names the shell registers: the last path segment of each `generate_handler!` entry. */
export function registeredCommands(mainRs: string): string[] {
  const block = /generate_handler!\[([\s\S]*?)\]\)/.exec(mainRs)?.[1] ?? "";
  return block
    .split("\n")
    .map((line) => line.replace(/\/\/.*$/, "").trim())
    .flatMap((line) => line.split(","))
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0)
    .map((entry) => entry.split("::").at(-1) ?? entry)
    .sort();
}

describe("desktop platform ↔ shell command surface", () => {
  const platformSource = readFileSync(resolve(here, "tauri.ts"), "utf8");
  const mainRs = readFileSync(resolve(repoRoot, "desktop/src-tauri/src/main.rs"), "utf8");

  it("parses both sides", () => {
    expect(invokedCommands(platformSource).length).toBeGreaterThan(10);
    expect(registeredCommands(mainRs)).toContain("daemon_discover");
  });

  it("invokes only commands the shell registers", () => {
    const registered = new Set(registeredCommands(mainRs));
    const missing = invokedCommands(platformSource).filter((name) => !registered.has(name));
    expect(missing).toEqual([]);
  });
});
