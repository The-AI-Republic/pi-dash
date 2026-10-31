// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Palette account, utility and help entries (shell spec, SHELL-089).
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../clipboard.js", () => ({ copyCurrentPageUrl: vi.fn().mockResolvedValue(undefined) }));

import type { PidashCommand } from "../../commands/index.js";
import { copyCurrentPageUrl } from "../clipboard.js";
import { SIDEBAR_DEFAULT_WIDTH, useShellChrome } from "../chromeState.js";
import { useShortcutsDialog } from "../shortcutsDialogStore.js";
import { useTopSearchFocus } from "../topSearchFocus.js";
import { DOCS_URL, FORUM_URL, REPORT_BUG_URL, buildAccountCommands } from "./account.js";
import {
  DOCS_URL as LEAF_DOCS_URL,
  FORUM_URL as LEAF_FORUM_URL,
  REPORT_BUG_URL as LEAF_REPORT_BUG_URL,
} from "../externalLinks.js";

afterEach(() => {
  useShellChrome.setState({ sidebarCollapsed: false, peekOpen: false, sidebarWidth: SIDEBAR_DEFAULT_WIDTH });
  useTopSearchFocus.setState({ request: 0 });
  useShortcutsDialog.getState().closeShortcuts();
  vi.mocked(copyCurrentPageUrl).mockClear();
});

function byId(commands: PidashCommand[]): Map<string, PidashCommand> {
  return new Map(commands.map((command) => [command.id, command]));
}

describe("account entries", () => {
  it("lists the account, utility and help entries with their chords", () => {
    const commands = buildAccountCommands({ onWorkspaceInvites: () => undefined, onOpenExternal: () => undefined });
    expect(commands.map((command) => [command.id, command.title, command.section, command.shortcut])).toEqual([
      ["shell.workspace-invites", "Workspace invites", "Account", undefined],
      ["shell.toggle-sidebar", "Toggle app sidebar", "Miscellaneous", "mod+b"],
      ["shell.copy-page-url", "Copy current page URL", "Miscellaneous", "mod+shift+c"],
      ["shell.focus-search", "Focus search input", "Miscellaneous", "mod+f"],
      ["shell.open-shortcuts", "Open keyboard shortcuts", "Help", "mod+/"],
      ["shell.docs", "Open Pi Dash documentation", "Help", undefined],
      ["shell.forum", "Join our Forum", "Help", undefined],
      ["shell.report-bug", "Report a bug", "Help", undefined],
    ]);
  });

  it("routes invites, toggles the sidebar, copies, focuses and opens the viewer", () => {
    const onWorkspaceInvites = vi.fn();
    const entries = byId(buildAccountCommands({ onWorkspaceInvites, onOpenExternal: () => undefined }));
    entries.get("shell.workspace-invites")!.run();
    expect(onWorkspaceInvites).toHaveBeenCalledTimes(1);
    useShellChrome.setState({ peekOpen: true });
    entries.get("shell.toggle-sidebar")!.run();
    expect(useShellChrome.getState().sidebarCollapsed).toBe(true);
    expect(useShellChrome.getState().peekOpen).toBe(false);
    entries.get("shell.copy-page-url")!.run();
    expect(copyCurrentPageUrl).toHaveBeenCalledTimes(1);
    entries.get("shell.focus-search")!.run();
    expect(useTopSearchFocus.getState().request).toBe(1);
    entries.get("shell.open-shortcuts")!.run();
    expect(useShortcutsDialog.getState().open).toBe(true);
  });

  it("opens the help links outside the app", () => {
    const onOpenExternal = vi.fn();
    const entries = byId(buildAccountCommands({ onWorkspaceInvites: () => undefined, onOpenExternal }));
    entries.get("shell.docs")!.run();
    entries.get("shell.forum")!.run();
    entries.get("shell.report-bug")!.run();
    expect(onOpenExternal.mock.calls.map((call) => call[0])).toEqual([DOCS_URL, FORUM_URL, REPORT_BUG_URL]);
  });

  it("mirrors the eager-only leaf destinations exactly", () => {
    expect([DOCS_URL, FORUM_URL, REPORT_BUG_URL]).toEqual([LEAF_DOCS_URL, LEAF_FORUM_URL, LEAF_REPORT_BUG_URL]);
  });
});
