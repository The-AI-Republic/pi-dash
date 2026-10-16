// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Source-level absence pin (ADM-073, from
// parity/specs/admin.md §7.10): no god-mode module may wire a
// workspace-deletion affordance. Backstop behind the detail page's
// zero-dialog pin: it covers every admin file, including screens
// this slice never mounts (the workspace pages land with
// NEWFRONT-512/513 and must keep passing). Fixtures below prove the
// matcher flags deletion identifiers while passing the loop delete,
// target cells, and prose.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));

/** Comment-free body: prose may name the absence, code may not. */
function scanBody(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[\s;])\/\/[^\n]*/g, "$1");
}

/**
 * Identifiers joining the workspace and deletion concepts
 * (case-insensitive, either order, any affixes). Single-concept
 * names — the loop delete, workspace cells and slugs — pass.
 */
function workspaceDeleteHits(body: string): string[] {
  const tokens = body.match(/[$A-Z_a-z][$\w]*/g) ?? [];
  return tokens.filter((token) => /workspace/i.test(token) && /delet|remov|destroy/i.test(token));
}

function adminSources(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) {
      out.push(...adminSources(full));
    } else if (!name.endsWith(".test.ts") && !name.endsWith(".test.tsx")) {
      out.push(full);
    }
  }
  return out;
}

function godModeSources(): string[] {
  return [...adminSources(here), ...adminSources(join(here, "..", "..", "routes", "god-mode"))];
}

describe("admin workspace-deletion scan (ADM-073)", () => {
  it("flags workspace-deletion identifiers", () => {
    expect(workspaceDeleteHits(scanBody("await deleteWorkspace(id)"))).toEqual(["deleteWorkspace"]);
    expect(workspaceDeleteHits(scanBody("const remove = useDeleteWorkspace();"))).toEqual(["useDeleteWorkspace"]);
    expect(workspaceDeleteHits(scanBody("<WorkspaceDeleteButton />"))).toEqual(["WorkspaceDeleteButton"]);
    expect(workspaceDeleteHits(scanBody("removeWorkspaceById(slug)"))).toEqual(["removeWorkspaceById"]);
    expect(workspaceDeleteHits(scanBody("workspaceDelete(slug)"))).toEqual(["workspaceDelete"]);
    expect(workspaceDeleteHits(scanBody("destroyWorkspace(job)"))).toEqual(["destroyWorkspace"]);
  });

  it("passes the loop delete, workspace cells, and prose", () => {
    expect(workspaceDeleteHits(scanBody("await deleteLoopJob(client, jobId)"))).toEqual([]);
    expect(workspaceDeleteHits(scanBody("const deletion = useDeleteLoopJob();"))).toEqual([]);
    expect(workspaceDeleteHits(scanBody("{target.workspace_slug}"))).toEqual([]);
    expect(workspaceDeleteHits(scanBody("workspaceSlug: row.slug"))).toEqual([]);
    expect(workspaceDeleteHits(scanBody("<FirstWorkspacePopup />"))).toEqual([]);
    expect(workspaceDeleteHits(scanBody("// workspace deletion is not offered here"))).toEqual([]);
    expect(workspaceDeleteHits(scanBody("/* never delete a workspace */"))).toEqual([]);
    expect(workspaceDeleteHits(scanBody('"Workspaces cannot be deleted here"'))).toEqual([]);
  });

  it("finds no workspace-deletion affordance in god-mode sources", () => {
    const files = godModeSources();
    expect(files.length).toBeGreaterThan(0);
    expect(files.some((file) => file.endsWith("AdminLoopJobDetailPage.tsx"))).toBe(true);
    expect(files.some((file) => file.includes(join("routes", "god-mode")))).toBe(true);
    for (const file of files) {
      expect(workspaceDeleteHits(scanBody(readFileSync(file, "utf8"))), file).toEqual([]);
    }
  });
});
