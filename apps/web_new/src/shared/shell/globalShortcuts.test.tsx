// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Global shortcut firing (shell spec, SHELL-080/086/087/089/092).
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearCommands,
  registerCommands,
  registerPalettePages,
  clearPalettePages,
  useCommandPalette,
} from "../commands/index.js";
import { SEQUENCE_WINDOW_MS, useGlobalShortcuts } from "./globalShortcuts.js";

function Probe() {
  useGlobalShortcuts();
  return <input aria-label="typing surface" />;
}

function press(target: Element | Window, init: KeyboardEventInit): boolean {
  const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event.defaultPrevented;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  clearCommands();
  clearPalettePages();
  useCommandPalette.getState().closePalette();
});

describe("useGlobalShortcuts", () => {
  it("toggles the palette chord from anywhere, even while typing", () => {
    render(<Probe />);
    const input = screen.getByLabelText("typing surface");
    expect(press(input, { key: "k", metaKey: true })).toBe(true);
    expect(useCommandPalette.getState().open).toBe(true);
    expect(press(window, { key: "K", ctrlKey: true })).toBe(true);
    expect(useCommandPalette.getState().open).toBe(false);
  });

  it("fires registered chords outside typing surfaces", () => {
    const run = vi.fn();
    const unregister = registerCommands([
      { id: "shell.toggle-sidebar", title: "Toggle app sidebar", shortcut: "mod+b", run },
    ]);
    try {
      render(<Probe />);
      expect(press(window, { key: "b", metaKey: true })).toBe(true);
      expect(run).toHaveBeenCalledTimes(1);
      // Typing surfaces stay silent for every chord but the palette one.
      expect(press(screen.getByLabelText("typing surface"), { key: "b", metaKey: true })).toBe(false);
      expect(run).toHaveBeenCalledTimes(1);
      // Unmatched chords do nothing.
      expect(press(window, { key: "z", metaKey: true })).toBe(false);
    } finally {
      unregister();
    }
  });

  it("opens the palette onto the page for page-bound chords", () => {
    const run = vi.fn();
    const unregisterCommands = registerCommands([
      {
        id: "shell.open-project",
        title: "Open a project",
        shortcut: "mod+shift+o",
        page: "shell.project-picker",
        run,
      },
    ]);
    const unregisterPages = registerPalettePages([
      {
        id: "shell.project-picker",
        placeholder: "Open a project",
        usePageState: () => ({ status: "ready", options: [], emptyText: "none" }),
      },
    ]);
    try {
      render(<Probe />);
      press(window, { key: "o", metaKey: true, shiftKey: true });
      expect(useCommandPalette.getState().open).toBe(true);
      expect(useCommandPalette.getState().page).toBe("shell.project-picker");
      expect(run).not.toHaveBeenCalled();
    } finally {
      unregisterCommands();
      unregisterPages();
    }
  });

  it("fires two-key sequences case-insensitively", () => {
    const run = vi.fn();
    const unregister = registerCommands([{ id: "shell.go-home", title: "Go to home", sequence: "gh", run }]);
    try {
      render(<Probe />);
      expect(press(window, { key: "G" })).toBe(false);
      expect(run).not.toHaveBeenCalled();
      expect(press(window, { key: "h" })).toBe(true);
      expect(run).toHaveBeenCalledTimes(1);
    } finally {
      unregister();
    }
  });

  it("opens the palette onto the page for sequenced pickers", () => {
    const unregisterCommands = registerCommands([
      {
        id: "shell.open-project",
        title: "Open a project",
        sequence: "op",
        page: "shell.project-picker",
        run: () => undefined,
      },
    ]);
    const unregisterPages = registerPalettePages([
      {
        id: "shell.project-picker",
        placeholder: "Open a project",
        usePageState: () => ({ status: "ready", options: [], emptyText: "none" }),
      },
    ]);
    try {
      render(<Probe />);
      press(window, { key: "o" });
      press(window, { key: "p" });
      expect(useCommandPalette.getState().open).toBe(true);
      expect(useCommandPalette.getState().page).toBe("shell.project-picker");
    } finally {
      unregisterCommands();
      unregisterPages();
    }
  });

  it("fires a bare single-key chord as the first accumulated key", () => {
    const run = vi.fn();
    const unregister = registerCommands([{ id: "ctx.state", title: "Change state", shortcut: "s", run }]);
    try {
      render(<Probe />);
      expect(press(window, { key: "s" })).toBe(true);
      expect(run).toHaveBeenCalledTimes(1);
    } finally {
      unregister();
    }
  });

  it("prefers a two-key sequence over a bare chord on its second key", () => {
    const sequenced = vi.fn();
    const chorded = vi.fn();
    const unregister = registerCommands([
      { id: "shell.open-project", title: "Open a project", sequence: "op", run: sequenced },
      { id: "ctx.priority", title: "Change priority", shortcut: "p", run: chorded },
    ]);
    try {
      render(<Probe />);
      press(window, { key: "o" });
      expect(press(window, { key: "p" })).toBe(true);
      expect(sequenced).toHaveBeenCalledTimes(1);
      expect(chorded).not.toHaveBeenCalled();
    } finally {
      unregister();
    }
  });

  it("never fires a bare chord as the second accumulated key", () => {
    const run = vi.fn();
    const unregister = registerCommands([{ id: "ctx.state", title: "Change state", shortcut: "s", run }]);
    try {
      render(<Probe />);
      press(window, { key: "x" });
      press(window, { key: "s" });
      expect(run).not.toHaveBeenCalled();
    } finally {
      unregister();
    }
  });

  it("forgets partial sequences after the idle window", () => {
    const run = vi.fn();
    const unregister = registerCommands([{ id: "shell.go-home", title: "Go to home", sequence: "gh", run }]);
    try {
      render(<Probe />);
      press(window, { key: "g" });
      vi.advanceTimersByTime(SEQUENCE_WINDOW_MS);
      press(window, { key: "h" });
      expect(run).not.toHaveBeenCalled();
      press(window, { key: "g" });
      press(window, { key: "h" });
      expect(run).toHaveBeenCalledTimes(1);
    } finally {
      unregister();
    }
  });

  it("ignores shifted keys, long keys and unregistered sequences", () => {
    const run = vi.fn();
    const unregister = registerCommands([{ id: "shell.go-home", title: "Go to home", sequence: "gh", run }]);
    try {
      render(<Probe />);
      expect(press(window, { key: "G", shiftKey: true })).toBe(false);
      expect(press(window, { key: "Shift" })).toBe(false);
      press(window, { key: "z" });
      press(window, { key: "z" });
      expect(run).not.toHaveBeenCalled();
    } finally {
      unregister();
    }
  });

  it("ignores control- and alt-modified keys without feeding the sequence", () => {
    const run = vi.fn();
    const unregister = registerCommands([{ id: "shell.go-home", title: "Go to home", sequence: "gh", run }]);
    try {
      render(<Probe />);
      press(window, { key: "g" });
      expect(press(window, { key: "q", ctrlKey: true })).toBe(false);
      expect(press(window, { key: "y", altKey: true })).toBe(false);
      expect(run).not.toHaveBeenCalled();
      press(window, { key: "h" });
      expect(run).toHaveBeenCalledTimes(1);
    } finally {
      unregister();
    }
  });

  it("detaches on unmount", () => {
    const run = vi.fn();
    const unregister = registerCommands([
      { id: "shell.toggle-sidebar", title: "Toggle app sidebar", shortcut: "mod+b", run },
    ]);
    try {
      const { unmount } = render(<Probe />);
      unmount();
      press(window, { key: "b", metaKey: true });
      expect(run).not.toHaveBeenCalled();
    } finally {
      unregister();
    }
  });
});

describe("useGlobalShortcuts page shortcuts", () => {
  function PageProbe() {
    useGlobalShortcuts({ pageShortcuts: false });
    return <div />;
  }

  it("keeps the palette chord but drops chords, sequences and handled keys", () => {
    const run = vi.fn();
    const unregister = registerCommands([
      { id: "shell.toggle-sidebar", title: "Toggle app sidebar", shortcut: "mod+b", run },
      { id: "shell.go-home", title: "Go to home", sequence: "gh", run },
    ]);
    try {
      render(<PageProbe />);
      expect(press(window, { key: "b", metaKey: true })).toBe(false);
      expect(press(window, { key: "g" })).toBe(false);
      expect(press(window, { key: "h" })).toBe(false);
      // Shift-modified keys take the same early exit.
      expect(press(window, { key: "B", metaKey: true, shiftKey: true })).toBe(false);
      expect(run).not.toHaveBeenCalled();
      expect(press(window, { key: "k", metaKey: true })).toBe(true);
      expect(useCommandPalette.getState().open).toBe(true);
    } finally {
      unregister();
    }
  });
});
