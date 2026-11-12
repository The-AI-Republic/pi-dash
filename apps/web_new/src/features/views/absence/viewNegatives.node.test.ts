// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Source-level absence pins (VIEW-043, VIEW-044, VIEW-046, VIEW-047,
// from parity/specs/views.md §13): no views module may poll, stream
// or subscribe (043), register key handlers outside the two search
// fields or palette sequences outside the palette (044), touch a
// desktop-only platform member (046), or add drag or file/export
// affordances (047). Backstop behind the DOM auditors and the query
// option pins: it covers every file, including screens no render test
// mounts. Fixtures below prove each token family flags real usage
// while passing the auditors' own oracle-mirroring selectors.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const SRC = join(here, "..", "..", "..");

/** Comment-free body: prose may name the absences, code may not. */
function scanBody(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[\s;])\/\/[^\n]*/g, "$1");
}

/** Body with the one allowed polling mention removed (see below). */
function liveBody(source: string): string {
  return scanBody(source).replace(/\brefetchInterval\s*:\s*false\b/g, "");
}

/** Body with the explicit drag opt-out removed (see below). */
function transferBody(source: string): string {
  return scanBody(source)
    .replace(/\bdraggable\s*=\s*\{\s*false\s*\}/g, "")
    .replace(/\bdraggable\s*=\s*"false"/g, "");
}

function hits(source: string, tokens: RegExp[]): string[] {
  return tokens.filter((token) => token.test(source)).map((token) => token.source);
}

/** Live-update transports and polling (VIEW-043). */
const LIVE_TOKENS = [
  /\bWebSocket\b/,
  /\bEventSource\b/,
  /\bRealtimeHub\b/,
  /createRealtimeHub/,
  /\bsetInterval\b/,
  /\brefetchInterval\b/,
  /\brefetchOnWindowFocus\b/,
  /\brefetchOnReconnect\b/,
  /\brefetchOnMount\b/,
  /\bonFocusChange\b/,
  /["']focus["']/,
  /\bsubscribe\s*\(/,
];

/** React key props and key-event listener names (VIEW-044). */
const KEY_TOKENS = [/\bonKey(?:Down|Up|Press)\b/, /["']key(?:down|up|press)["']/];

/**
 * Desktop-only platform members and desktop module paths (VIEW-046).
 * The shared `platform.openExternal` / `platform.shareableUrl` pair the
 * new-tab entries use is pinned separately below, not here.
 */
const DESKTOP_TOKENS = [
  /\bagentRuntime\b/,
  /\blocalChat\b/,
  /\bdeepLinks\b/,
  /\bcliSetup\b/,
  /\bshellSignIn\b/,
  /\bsessionTeardown\b/,
  /\bAgentRuntimeApi\b/,
  /\bLocalChatApi\b/,
  /\bDeepLinkApi\b/,
  /\bCliSetupApi\b/,
  /\bShellSignInApi\b/,
  /\bSessionTeardownApi\b/,
  /\bTitleBarApi\b/,
  /\bMenuApi\b/,
  /\bMenuCommand\b/,
  /\bUpdaterApi\b/,
  /platform-target/,
  /\/tauri\.js/,
  /__TAURI__/,
];

/**
 * Native drag affordances, file choosers and export/import/print
 * controls (VIEW-047). The tag- and quote-anchored forms match JSX
 * usage while passing the auditors' own query selectors
 * (`'[draggable="true"]'`, `'input[type="file"]'`), which carry no
 * opening tag, and the `export`/`import` keywords, which carry no
 * quote. `draggable={false}` is stripped before the scan: opting an
 * illustration out of dragging is the absence, not a violation.
 */
const TRANSFER_TOKENS = [
  /<\w[^>]*\bdraggable\b/,
  /<\s*input[^>]*\btype\s*=\s*["']file["']/,
  /\btype\s*=\s*\{\s*["']file["']/,
  /\bonDrag\w*\s*=/,
  /\bonDrop\s*=/,
  /\b[Dd]ataTransfer\b/,
  /pragmatic-drag-and-drop/,
  /["'`]Export\b/,
  /["'`]Import\b/,
  /["'`]Print\b/,
  />\s*Export\b/,
  />\s*Import\b/,
  />\s*Print\b/,
  /\bFileDown\b/,
  /\bFileUp\b/,
  /\bPrinter\b/,
  /\bDownload\b/,
  /\bUpload\b/,
  /window\.print/,
];

function sourcesUnder(directory: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(directory)) {
    const full = join(directory, name);
    if (statSync(full).isDirectory()) {
      out.push(...sourcesUnder(full));
    } else if (/\.(ts|tsx)$/.test(name) && !/\.test\.tsx?$/.test(name)) {
      out.push(full);
    }
  }
  return out;
}

/** Every views source: the feature plus the routes that mount it. */
const entries = [
  ...sourcesUnder(join(SRC, "features", "views")),
  ...sourcesUnder(join(SRC, "routes", "$ws", "_gated", "projects", "$projectId", "views")),
  ...sourcesUnder(join(SRC, "routes", "$ws", "_gated", "workspace-views")),
  join(SRC, "routes", "$ws", "_gated", "settings", "projects", "$projectId", "features", "views.tsx"),
];

const short = (path: string): string => relative(SRC, path);

describe("views scan scope", () => {
  it("covers the feature, its routes and the auditor itself", () => {
    expect(entries.length).toBeGreaterThan(50);
    expect(entries.some((file) => file.endsWith(join("features", "views", "api", "queries.ts")))).toBe(true);
    expect(entries.some((file) => file.endsWith(join("features", "views", "api", "detail.ts")))).toBe(true);
    expect(entries.some((file) => file.endsWith(join("features", "views", "absence", "viewAbsence.ts")))).toBe(true);
    expect(entries.some((file) => file.endsWith(join("views", "index.tsx")))).toBe(true);
    expect(entries.some((file) => file.endsWith(join("workspace-views", "index.tsx")))).toBe(true);
    expect(entries.some((file) => file.endsWith(join("features", "views.tsx")))).toBe(true);
  });
});

describe("views live-update scan (VIEW-043)", () => {
  it("flags transports, polling and focus/subscribe hooks", () => {
    expect(hits(liveBody("new WebSocket(url)"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("new EventSource(url)"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("createRealtimeHub()"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("setInterval(tick, 1000)"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("queryOptions({ refetchInterval: 5000 })"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("queryOptions({ refetchOnWindowFocus: true })"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("platform.onFocusChange(refetch)"), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody('window.addEventListener("focus", refetch)'), LIVE_TOKENS)).not.toEqual([]);
    expect(hits(liveBody("store.subscribe(render)"), LIVE_TOKENS)).not.toEqual([]);
  });

  it("passes the explicit disabled interval, focus calls and prose", () => {
    expect(hits(liveBody("queryOptions({ refetchInterval: false })"), LIVE_TOKENS)).toEqual([]);
    expect(hits(liveBody("titleRef.current!.focus()"), LIVE_TOKENS)).toEqual([]);
    expect(hits(liveBody("// polling and subscriptions stay out of views"), LIVE_TOKENS)).toEqual([]);
  });

  it("finds no live-update usage in views sources", () => {
    for (const file of entries) {
      expect(hits(liveBody(readFileSync(file, "utf8")), LIVE_TOKENS), short(file)).toEqual([]);
    }
  });
});

describe("views key-handler scan (VIEW-044)", () => {
  it("flags key props and key-event listeners", () => {
    expect(hits(scanBody("<div onKeyDown={close}>x</div>"), KEY_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('addEventListener("keydown", onKey)'), KEY_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("target.addEventListener('keyup', onKey)"), KEY_TOKENS)).not.toEqual([]);
  });

  it("passes non-key listeners and prose", () => {
    expect(hits(scanBody('document.addEventListener("pointerdown", tap)'), KEY_TOKENS)).toEqual([]);
    expect(hits(scanBody("// onKeyDown stays out of the dialogs"), KEY_TOKENS)).toEqual([]);
  });

  it("keeps key handling in the two search fields only", () => {
    const keyed = entries.filter((file) => hits(scanBody(readFileSync(file, "utf8")), KEY_TOKENS).length > 0);
    expect(keyed.map(short).sort()).toEqual(
      [
        join("features", "views", "components", "ProjectViewsSearch.tsx"),
        join("features", "views", "components", "WorkspaceViewsSearch.tsx"),
      ].sort()
    );
  });

  it("guards both search handlers on Escape", () => {
    for (const name of ["ProjectViewsSearch.tsx", "WorkspaceViewsSearch.tsx"]) {
      const body = scanBody(readFileSync(join(SRC, "features", "views", "components", name), "utf8"));
      expect(body.includes('!== "Escape"'), name).toBe(true);
    }
  });

  it("registers palette sequences only in the palette builders", () => {
    const sequenced = entries.filter((file) => /\bsequence\b/.test(scanBody(readFileSync(file, "utf8"))));
    expect(sequenced.map(short)).toEqual([join("features", "views", "palette", "viewCommands.ts")]);
  });
});

describe("views desktop scan (VIEW-046)", () => {
  it("flags desktop-only members and module paths", () => {
    expect(hits(scanBody("platform.agentRuntime?.isAvailable()"), DESKTOP_TOKENS)).not.toEqual([]);
    expect(hits(scanBody("platform.localChat.listSessions(a)"), DESKTOP_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('import { platform } from "@pidash/platform-target"'), DESKTOP_TOKENS)).not.toEqual([]);
    expect(hits(scanBody('import { x } from "../tauri.js"'), DESKTOP_TOKENS)).not.toEqual([]);
  });

  it("passes the shared open/share pair and prose", () => {
    expect(hits(scanBody("platform.openExternal(platform.shareableUrl(href))"), DESKTOP_TOKENS)).toEqual([]);
    expect(hits(scanBody("// no desktop runtime on the views surface"), DESKTOP_TOKENS)).toEqual([]);
  });

  it("finds no desktop-only usage in views sources", () => {
    for (const file of entries) {
      expect(hits(scanBody(readFileSync(file, "utf8")), DESKTOP_TOKENS), short(file)).toEqual([]);
    }
  });

  it("touches only the shared open/share platform members", () => {
    const members = new Set<string>();
    for (const file of entries) {
      const body = scanBody(readFileSync(file, "utf8"));
      for (const match of body.matchAll(/platform\.([A-Za-z]+)/g)) {
        members.add(match[1] ?? "");
      }
    }
    for (const member of members) {
      expect(["openExternal", "shareableUrl"]).toContain(member);
    }
  });
});

describe("views transfer scan (VIEW-047)", () => {
  it("flags drag, file and export/import/print usage", () => {
    expect(hits(transferBody('<div draggable="true">row</div>'), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody("<li draggable={canMove}>row</li>"), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody('<input type="file" />'), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody('<input type={"file"} />'), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody("<div onDrop={drop}>zone</div>"), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody("event.dataTransfer.files"), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody("<Button>Export CSV</Button>"), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody('<a aria-label="Import workspace">go</a>'), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody("<Printer size={14} />"), TRANSFER_TOKENS)).not.toEqual([]);
    expect(hits(transferBody("window.print()"), TRANSFER_TOKENS)).not.toEqual([]);
  });

  it("passes the drag opt-out, the auditors' own selectors and prose", () => {
    expect(hits(transferBody('<img draggable={false} alt="" />'), TRANSFER_TOKENS)).toEqual([]);
    expect(hits(transferBody("root.querySelectorAll('[draggable=\"true\"]')"), TRANSFER_TOKENS)).toEqual([]);
    expect(hits(transferBody("root.querySelectorAll('input[type=\"file\"]')"), TRANSFER_TOKENS)).toEqual([]);
    expect(hits(transferBody("export function auditViewAbsence() {}"), TRANSFER_TOKENS)).toEqual([]);
    expect(hits(transferBody('import { x } from "./y.js"'), TRANSFER_TOKENS)).toEqual([]);
    expect(hits(transferBody("// drag and exports stay out of views"), TRANSFER_TOKENS)).toEqual([]);
  });

  it("finds no drag or transfer affordances in views sources", () => {
    for (const file of entries) {
      expect(hits(transferBody(readFileSync(file, "utf8")), TRANSFER_TOKENS), short(file)).toEqual([]);
    }
  });
});
