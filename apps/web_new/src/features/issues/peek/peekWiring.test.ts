// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Peek wiring conformance (parity/specs/issues.md §14, ISS-165/167). The
// peek address used to be an optional per-layout prop triple, and each
// layout that missed it kept the peek to itself without failing a test.
// These checks own the cross-layout invariant instead: every issues layout
// takes its address from the one shared contract, derives it from the one
// shared hook, and every host that mounts layouts forwards that address.
// The workspace all-issues body is an issues surface too, so it is held to
// the same contract, and both workspace routes must declare the address.
// A new layout, body or host that skips the wiring fails here.

import * as fs from "node:fs";
import * as path from "node:path";
import { describe, expect, it } from "vitest";

const ISSUES = path.resolve(__dirname, "..");

/** The five layouts an issues surface can open, with their source files. */
const LAYOUTS: readonly { name: string; file: string }[] = [
  { name: "list", file: "list/ListLayout.tsx" },
  { name: "board", file: "kanban/KanbanBoard.tsx" },
  { name: "calendar", file: "calendar/CalendarLayout.tsx" },
  { name: "spreadsheet", file: "spreadsheet/SpreadsheetLayout.tsx" },
  { name: "gantt", file: "gantt/GanttLayout.tsx" },
];

function source(file: string): string {
  return fs.readFileSync(path.join(ISSUES, file), "utf8");
}

const MOUNT_TAG =
  /^\s*<(?:Lazy)?(?:Resolved)?(?:ListLayout|KanbanBoard|CalendarEntry|CalendarLayout|SpreadsheetLayout|GanttTimeline|GanttLayout|KanbanLayout)\b/gm;

/**
 * Each layout mount in a file, as the JSX from its tag to the next mount.
 * A tag opens a line (so `return <X` and `<X` both count) and a property
 * reference such as `module.GanttTimeline` never does, so lazy loaders and
 * type imports stay out.
 */
function layoutMounts(file: string): { line: number; body: string }[] {
  const lines = source(file).split("\n");
  const starts: number[] = [];
  lines.forEach((line, index) => {
    if (MOUNT_TAG.test(line)) starts.push(index);
    MOUNT_TAG.lastIndex = 0;
  });
  return starts.map((index, position) => {
    const end = starts[position + 1] ?? Math.min(lines.length, index + 40);
    return { line: index + 1, body: lines.slice(index, end).join("\n") };
  });
}

describe("peek wiring", () => {
  it.each(LAYOUTS)("$name takes its address from the shared contract", ({ file }) => {
    const text = source(file);
    // One props type owns the seam; a layout that declares its own optional
    // peek props can silently opt out of the address.
    expect(text).toMatch(/interface \w+Props extends LayoutPeekProps \{/);
    expect(text).not.toMatch(/^\s*peekKey\?: /m);
    // One implementation of the state machine, not one per layout.
    expect(text).toMatch(/usePeekAddress\(\{/);
    expect(text).toMatch(/peek\.(peeked|open|close)\b/);
  });

  it("the shared contract is declared once", () => {
    const declaring = LAYOUTS.filter(({ file }) => /interface LayoutPeekProps\b/.test(source(file)));
    expect(declaring).toHaveLength(0);
    expect(source("peek/usePeekAddress.ts")).toMatch(/export interface LayoutPeekProps \{/);
  });

  it("a layout never keeps its own peek state or writes the address itself", () => {
    for (const { name, file } of LAYOUTS) {
      const text = source(file);
      // Peek selection held by the layout is the drift that let each copy
      // of the state machine keep a bug the others had already fixed.
      const peekState = text.split("\n").filter((line) => /useState/.test(line) && /peek/i.test(line));
      expect(peekState, name).toEqual([]);
      // Address writes belong to the hook, so every layout replaces alike.
      expect(text, name).not.toMatch(/replacePeekSearch|history\.replaceState/);
    }
  });

  it.each(LAYOUTS)("$name reports nested opens and child closes through the hook", ({ file }) => {
    const text = source(file);
    // A layout that forwards only a host writer swallows the panel's report
    // on a standalone mount, so a nested open never reaches the address.
    expect(text).toMatch(/<LazyIssuePeek\b/);
    expect(text).toMatch(/onPeekChange=\{peek\.report\}/);
  });

  it("the workspace all-issues body takes the same contract and stream", () => {
    const text = source("global/GlobalIssuesScreen.tsx");
    // The body used to be a flat list of browse links with no address; a
    // body that keeps its own peek state would repeat the original drift.
    expect(text).toMatch(/interface GlobalIssuesScreenProps extends LayoutPeekProps \{/);
    expect(text).toMatch(/usePeekAddress\(\{ projectId/);
    expect(text).not.toMatch(/replacePeekSearch|history\.replaceState/);
    expect(text.split("\n").filter((line) => /useState/.test(line) && /peek/i.test(line))).toEqual([]);
    // Rows open through the address and the panel reports back into it.
    expect(text).toMatch(/peek\.open\(/);
    expect(text).toMatch(/<LazyIssuePeek\b/);
    expect(text).toMatch(/onPeekChange=\{peek\.report\}/);
    expect(text).toMatch(/onClose=\{peek\.close\}/);
    // Its writer spells the project on every write (there is no route
    // project to omit), so the mount's address basis is "" — not the
    // project it reads under, which would make a nested open re-root.
    expect(text).toMatch(/addressProjectId=""/);
  });

  it.each([
    "../../routes/$ws/_gated/workspace-views/all-issues.tsx",
    "../../routes/$ws/_gated/workspace-views/$viewId/index.tsx",
  ])("%s declares the peek address and hands it to the workspace screen", (route) => {
    const text = source(route);
    // Without a validated search the route cannot read a shared link, and
    // without the writer every open would push history instead of replace.
    expect(text).toMatch(/validateSearch: \(search: Record<string, unknown>\) => parseIssueSearch\(search\)/);
    expect(text).toMatch(/<WorkspaceViewScreen\b[^>]*\bpeek=\{peek\}/);
    expect(text).toMatch(/search\.peekIssueId/);
    expect(text).toMatch(/peekSearchFor\(/);
    expect(text).toMatch(/stripPeekParams\(/);
    expect(text).toMatch(/replace: true/);
  });

  it.each([
    "layouts/ViewLayoutBody.tsx",
    "list/EntityListScreen.tsx",
    "../../routes/$ws/_gated/projects/$projectId/issues/IssuesListContent.tsx",
    "../../routes/$ws/_gated/projects/$projectId/cycles/CycleDetailContent.tsx",
  ])("%s hands the address to every layout it mounts", (host) => {
    const mounts = layoutMounts(host);
    expect(mounts.length).toBeGreaterThan(0);
    for (const mount of mounts) {
      // Either an explicit key or the one spread that carries it.
      expect(mount.body, `${host}:${mount.line}`).toMatch(/peekKey=\{|\{\.\.\.peekProps\}/);
    }
  });
});
