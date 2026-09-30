// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Tests for the similarity check's own-code exclusion (F-12 / NEWFRONT-102).
// Each test builds a hermetic fixture repo in a temp dir so it never depends on
// the real old-frontend bytes (and so this file itself is never a corpus match).
// The two fixture blocks below are synthetic and the same size class (>= 50
// tokens): the only difference between the own-code case and the Plane case is
// the path a copy is taken from, which is exactly what the exclusion decides.
import { afterAll, describe, expect, it } from "vitest";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { isOwnCode, ownCode, runSimilarityCheck } from "./check-similarity.mjs";

// A synthetic block that stands in for AI Republic's own code (runner logic).
const ownBlock = `export function buildRunnerSchedule(runs, options) {
  const pending = [];
  for (const run of runs) {
    if (run.status === "queued" && run.attempts < options.maxAttempts) {
      pending.push({ id: run.id, priority: run.priority ?? 0, queuedAt: Date.now() });
    }
  }
  pending.sort((left, right) => right.priority - left.priority);
  return pending.map((entry, slot) => ({ ...entry, slot, backoffMs: 1000 * (slot + 1) }));
}
`;

// A synthetic block that stands in for Plane code (an activity feed).
const planeBlock = `export function summarizeActivity(entries, viewer) {
  const grouped = new Map();
  for (const entry of entries) {
    const actorId = entry.actor?.id ?? "system";
    const bucket = grouped.get(actorId) ?? [];
    bucket.push({ verb: entry.verb, field: entry.field, at: entry.createdAt });
    grouped.set(actorId, bucket);
  }
  return Array.from(grouped.entries()).map(([id, items]) => ({ id, count: items.length, viewer }));
}
`;

const tempRoots = [];

function makeRepo(files) {
  const root = mkdtempSync(join(tmpdir(), "sim-check-"));
  tempRoots.push(root);
  for (const [rel, content] of Object.entries(files)) {
    const full = join(root, rel);
    mkdirSync(dirname(full), { recursive: true });
    writeFileSync(full, content);
  }
  return root;
}

afterAll(() => {
  for (const root of tempRoots) rmSync(root, { recursive: true, force: true });
});

describe("check-similarity own-code exclusion", () => {
  it("passes a port of own code but still fails a same-size port of Plane code", () => {
    const root = makeRepo({
      // Own code: excluded from the old corpus (packages/services/src/runner/**).
      "packages/services/src/runner/schedule.ts": ownBlock,
      // Plane code: stays in the old corpus.
      "apps/web/core/components/core/activity.tsx": planeBlock,
      // New-tree files: verbatim ports of each.
      "apps/web_new/src/features/runners/schedule.ts": ownBlock,
      "apps/web_new/src/features/activity/summary.ts": planeBlock,
    });

    const { offenders } = runSimilarityCheck({
      root,
      oldTrees: ["apps/web", "packages/services"],
      newTrees: ["apps/web_new"],
    });

    // The port of own code is not flagged: its source was excluded.
    expect(offenders.some((o) => o.newFile === "apps/web_new/src/features/runners/schedule.ts")).toBe(false);
    expect(offenders.some((o) => o.oldFile.startsWith("packages/services/src/runner/"))).toBe(false);

    // The port of Plane code is still flagged, against the Plane source.
    const flagged = offenders.find((o) => o.newFile === "apps/web_new/src/features/activity/summary.ts");
    expect(flagged).toBeDefined();
    expect(flagged.oldFile).toBe("apps/web/core/components/core/activity.tsx");
    expect(flagged.tokens).toBeGreaterThanOrEqual(50);
  });

  it("still flags the very same own-code block when it lives at a Plane path", () => {
    // Control: the block is detectable; only the path-based exclusion spares it.
    const root = makeRepo({
      // Same bytes as ownBlock, but NOT under an own-code path -> treated as Plane.
      "packages/services/src/misc-helpers.ts": ownBlock,
      "apps/web_new/src/features/runners/schedule.ts": ownBlock,
    });

    const { offenders } = runSimilarityCheck({
      root,
      oldTrees: ["packages/services"],
      newTrees: ["apps/web_new"],
    });

    const flagged = offenders.find((o) => o.newFile === "apps/web_new/src/features/runners/schedule.ts");
    expect(flagged).toBeDefined();
    expect(flagged.oldFile).toBe("packages/services/src/misc-helpers.ts");
    expect(flagged.tokens).toBeGreaterThanOrEqual(50);
  });

  it("isOwnCode matches every wiki own-code path and nothing else", () => {
    const own = [
      "apps/web/core/components/runners/runner-card.tsx",
      "apps/web/core/components/chat/message.tsx",
      "apps/web/core/components/assistant/panel.tsx",
      "apps/web/core/components/schedulers/list.tsx",
      "apps/web/core/components/agent-runtime.tsx",
      "apps/web/core/components/desktop-update-button.tsx",
      "apps/web/app/(all)/[workspaceSlug]/runners/page.tsx",
      "apps/web/app/(all)/[workspaceSlug]/schedulers/page.tsx",
      "apps/web/app/(all)/[workspaceSlug]/prompts/page.tsx",
      "apps/web/app/(all)/[workspaceSlug]/assistant/page.tsx",
      "apps/web/app/(all)/[workspaceSlug]/ai-dev-machines/page.tsx",
      "apps/web/app/(all)/[workspaceSlug]/(projects)/projects/(detail)/[projectId]/runners/page.tsx",
      "apps/web/app/(all)/[workspaceSlug]/(projects)/projects/(detail)/[projectId]/schedulers/page.tsx",
      "apps/web/app/(all)/[workspaceSlug]/(settings)/settings/projects/[projectId]/schedulers/page.tsx",
      "apps/web/core/store/scheduler.store.ts",
      "apps/web/core/store/prompt-section.store.ts",
      "apps/web/core/services/runner/pod.service.ts",
      "apps/web/core/services/agent-runtime.ts",
      "apps/web/core/services/desktop-session.ts",
      "packages/services/src/runner/index.ts",
      "packages/services/src/assistant/index.ts",
      "packages/services/src/scheduler/index.ts",
      "packages/services/src/prompt-section/index.ts",
      "packages/services/src/auto-pm/plan.ts",
      "packages/services/src/desktop-api-adapter.ts",
      "packages/services/src/desktop-event-source.ts",
      "desktop-overlay/src/main.ts",
    ];
    for (const path of own) expect(isOwnCode(path), path).toBe(true);

    const plane = [
      // Explicitly Plane per the wiki (present in the initial import).
      "apps/web/core/components/core/activity.tsx",
      "apps/web/core/components/automation/rule.tsx",
      "apps/web/ce/components/desktop/shell.tsx",
      "apps/web/core/store/issue.store.ts",
      "apps/web/core/services/project.service.ts",
      // Near-misses that must not be swept in by prefix matching.
      "packages/services/src/runner-legacy.ts",
      "packages/services/src/index.ts",
      "packages/ui/src/button.tsx",
      "apps/web_new/src/features/runners/schedule.ts",
    ];
    for (const path of plane) expect(isOwnCode(path), path).toBe(false);
  });

  it("every own-code entry carries a one-line reason", () => {
    expect(ownCode.length).toBeGreaterThan(0);
    for (const entry of ownCode) {
      expect(typeof entry.pattern).toBe("string");
      expect(entry.pattern.length).toBeGreaterThan(0);
      expect(typeof entry.reason).toBe("string");
      expect(entry.reason.trim().length).toBeGreaterThan(0);
      expect(entry.reason).not.toContain("\n");
    }
  });
});
