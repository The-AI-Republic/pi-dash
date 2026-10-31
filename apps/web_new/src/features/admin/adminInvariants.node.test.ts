// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Source-level absence pins (ADM-069–071, from
// parity/specs/admin.md §9): no admin module may register key
// handlers, add drag or file affordances, or reach for live-update
// transports. Backstop behind the DOM auditors: it covers every file,
// including screens no render test mounts. Fixtures below prove each
// token family flags real usage while passing the auditors' own
// oracle-mirroring selectors.
//
// The same scan carries the image-area absence (ADM-049/050): the
// console offers no third-party image search, so no admin module may
// name the Unsplash setting or an image-page skeleton.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));

/** Comment-free body: prose may name the absences, code may not. */
function scanBody(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[\s;])\/\/[^\n]*/g, "$1");
}

/** Body with the one allowed polling mention removed (see below). */
function liveBody(source: string): string {
  return scanBody(source).replace(/\brefetchInterval\s*:\s*false\b/g, "");
}

function hits(source: string, tokens: RegExp[]): string[] {
  return tokens.filter((token) => token.test(source)).map((token) => token.source);
}

/** React key props and key-event listener names. */
const KEY_TOKENS = [/\bonKey(?:Down|Up|Press)\b/, /["']key(?:down|up|press)["']/];

/**
 * Native drag affordances and file choosers. The tag-anchored forms
 * match JSX usage while passing the auditors' own query selectors
 * (`'[draggable="true"]'`, `'input[type="file"]'`), which carry no
 * opening tag.
 */
const TRANSFER_TOKENS = [
  /<\w[^>]*\bdraggable\b/,
  /<\s*input[^>]*\btype\s*=\s*["']file["']/,
  /\btype\s*=\s*\{\s*["']file["']/,
  /\bonDrag\w*\s*=/,
  /\bonDrop\s*=/,
  /\b[Dd]ataTransfer\b/,
  /pragmatic-drag-and-drop/,
];

/** Live-update transports and polling (ADM-069). */
const LIVE_TOKENS = [
  /\bWebSocket\b/,
  /\bEventSource\b/,
  /\bRealtimeHub\b/,
  /createRealtimeHub/,
  /\bsetInterval\b/,
  /\brefetchInterval\b/,
];

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

describe("admin key-handler scan (ADM-071)", () => {
  it("flags key props and key-event listeners", () => {
    expect(hits(scanBody("<div onKeyDown={close}>x</div>"), KEY_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('addEventListener("keydown", onKey)'), KEY_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("target.addEventListener('keyup', onKey)"), KEY_TOKENS)).not.toEqual([]);
  });

  it("passes non-key listeners and prose", () => {
    expect(hits(scanBody('window.addEventListener("resize", collapse)'), KEY_TOKENS)).toEqual([]);
    expect(hits(scanBody('document.addEventListener("pointerdown", tap)'), KEY_TOKENS)).toEqual([]);
    expect(hits(scanBody("// onKeyDown stays out of the console"), KEY_TOKENS)).toEqual([]);
  });

  it("finds no key registrations in admin sources", () => {
    for (const file of adminSources(here)) {
      expect(hits(scanBody(readFileSync(file, "utf8")), KEY_TOKENS), file).toEqual([]);
    }
  });
});

describe("admin transfer scan (ADM-070)", () => {
  it("flags drag and file usage", () => {
    expect(hits(scanBody('<div draggable="true">row</div>'), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("<li draggable={canMove}>row</li>"), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('<input type="file" />'), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('<input type={"file"} />'), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("<div onDrop={drop}>zone</div>"), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("event.dataTransfer.files"), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("new DataTransfer()"), TRANSFER_TOKENS)).not.toEqual([]);
  });

  it("passes the auditors' own selectors and prose", () => {
    expect(hits(scanBody("root.querySelectorAll('[draggable=\"true\"]')"), TRANSFER_TOKENS)).toEqual([]);
    expect(hits(scanBody("root.querySelectorAll('input[type=\"file\"]')"), TRANSFER_TOKENS)).toEqual([]);
    expect(hits(scanBody("// draggable and file choosers stay out"), TRANSFER_TOKENS)).toEqual([]);
  });

  it("finds no drag or file affordances in admin sources", () => {
    for (const file of adminSources(here)) {
      expect(hits(scanBody(readFileSync(file, "utf8")), TRANSFER_TOKENS), file).toEqual([]);
    }
  });
});

describe("admin live-update scan (ADM-069)", () => {
  it("flags transports and polling", () => {
    expect(hits(liveBody("new WebSocket(url)"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("new EventSource(url)"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("createRealtimeHub()"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("setInterval(tick, 1000)"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("queryOptions({ refetchInterval: 5000 })"), LIVE_TOKENS)).not.toEqual([]);
  });

  it("passes the explicit disabled interval and prose", () => {
    expect(hits(liveBody("queryOptions({ refetchInterval: false })"), LIVE_TOKENS)).toEqual([]);
    expect(hits(liveBody("// polling stays out of the console"), LIVE_TOKENS)).toEqual([]);
  });

  it("finds no live-update usage in admin sources", () => {
    const files = adminSources(here);
    expect(files.length).toBeGreaterThan(0);
    expect(files.some((file) => file.endsWith("queries.ts"))).toBe(true);
    for (const file of files) {
      expect(hits(liveBody(readFileSync(file, "utf8")), LIVE_TOKENS), file).toEqual([]);
    }
  });
});

/** The removed image area's surviving identifiers (ADM-049/050). */
const IMAGE_AREA_TOKENS = [/\bUNSPLASH_ACCESS_KEY\b/, /\badmin-image-skeleton\b/, /\badmin-nav-image\b/];

describe("admin image-area scan (ADM-049/050)", () => {
  it("flags the Unsplash key and the image-page skeleton ids", () => {
    expect(hits(scanBody('const key = "UNSPLASH_ACCESS_KEY";'), IMAGE_AREA_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('<div data-testid="admin-image-skeleton" />'), IMAGE_AREA_TOKENS)).not.toEqual([]);
    expect(
      hits(scanBody('<a data-testid="admin-nav-image" href="/god-mode/image/" />'), IMAGE_AREA_TOKENS)
    ).not.toEqual([]);
  });

  it("passes unrelated uses of the word image and prose", () => {
    expect(hits(scanBody('const logo: "image" | "initial" = kind;'), IMAGE_AREA_TOKENS)).toEqual([]);
    expect(hits(scanBody("const pageSize = 25;"), IMAGE_AREA_TOKENS)).toEqual([]);
    expect(hits(scanBody("// no image area: /image/ falls through"), IMAGE_AREA_TOKENS)).toEqual([]);
  });

  it("finds no image-area identifiers in admin sources", () => {
    for (const file of adminSources(here)) {
      expect(hits(scanBody(readFileSync(file, "utf8")), IMAGE_AREA_TOKENS), file).toEqual([]);
    }
  });
});
