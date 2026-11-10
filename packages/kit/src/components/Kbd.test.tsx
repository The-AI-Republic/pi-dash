// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { formatKbdKey, isMacPlatform, Kbd, kbdKeyLabel } from "./Kbd";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("formatKbdKey", () => {
  it("adapts modifier glyphs to the platform", () => {
    expect(["mod", "shift", "c"].map((key) => formatKbdKey(key, true))).toEqual(["⌘", "⇧", "C"]);
    expect(["mod", "shift", "c"].map((key) => formatKbdKey(key, false))).toEqual(["Ctrl", "Shift", "C"]);
    expect(["ctrl", "alt", "/"].map((key) => formatKbdKey(key, true))).toEqual(["⌃", "⌥", "/"]);
    expect(["mod", "k"].map((key) => formatKbdKey(key, false))).toEqual(["Ctrl", "K"]);
  });

  it("covers every modifier alias on both platforms", () => {
    expect(["meta", "cmd", "option"].map((key) => formatKbdKey(key, true))).toEqual(["⌘", "⌘", "⌥"]);
    expect(["ctrl", "alt"].map((key) => formatKbdKey(key, false))).toEqual(["Ctrl", "Alt"]);
  });

  it("renders unknown keys uppercased and blanks as Space instead of vanishing", () => {
    expect(formatKbdKey("bogus", false)).toBe("BOGUS");
    expect(formatKbdKey(" ", true)).toBe("Space");
  });
});

describe("kbdKeyLabel", () => {
  it("names the real keys on both platforms", () => {
    expect(["mod", "shift", "c"].map((key) => kbdKeyLabel(key, true))).toEqual(["Command", "Shift", "C"]);
    expect(["mod", "shift", "c"].map((key) => kbdKeyLabel(key, false))).toEqual(["Ctrl", "Shift", "C"]);
    expect(["ctrl", "alt", "/"].map((key) => kbdKeyLabel(key, true))).toEqual(["Control", "Option", "/"]);
    expect(["meta", "cmd", "option"].map((key) => kbdKeyLabel(key, true))).toEqual(["Command", "Command", "Option"]);
    expect(kbdKeyLabel("bogus", false)).toBe("BOGUS");
    expect(kbdKeyLabel(" ", false)).toBe("Space");
  });
});

describe("isMacPlatform", () => {
  it("detects macOS from the platform string", () => {
    vi.stubGlobal("navigator", { platform: "MacIntel" });
    expect(isMacPlatform()).toBe(true);
    vi.stubGlobal("navigator", { platform: "Win32" });
    expect(isMacPlatform()).toBe(false);
  });

  it("treats a missing navigator as non-Mac", () => {
    vi.stubGlobal("navigator", undefined);
    expect(isMacPlatform()).toBe(false);
  });

  it("treats a navigator without a platform string as non-Mac", () => {
    vi.stubGlobal("navigator", {});
    expect(isMacPlatform()).toBe(false);
  });
});

describe("Kbd", () => {
  it("names the full shortcut and shows each key", () => {
    vi.stubGlobal("navigator", { platform: "Win32" });
    render(<Kbd keys={["mod", "K"]} />);
    expect(screen.getByLabelText("Ctrl+K", { selector: "span" })).toBeInTheDocument();
    expect(screen.getByText("Ctrl", { selector: "kbd" })).toBeInTheDocument();
    expect(screen.getByText("K", { selector: "kbd" })).toBeInTheDocument();
  });

  it("renders macOS glyphs with spoken key names", () => {
    vi.stubGlobal("navigator", { platform: "MacIntel" });
    render(<Kbd keys={["mod", "K"]} />);
    expect(screen.getByLabelText("Command+K", { selector: "span" })).toBeInTheDocument();
    expect(screen.getByText("⌘", { selector: "kbd" })).toBeInTheDocument();
    expect(screen.getByText("K", { selector: "kbd" })).toBeInTheDocument();
  });

  it("maps every modifier in one chord on both platforms", () => {
    vi.stubGlobal("navigator", { platform: "Win32" });
    const { unmount } = render(<Kbd keys={["ctrl", "alt", "shift", "/"]} />);
    expect(screen.getByLabelText("Ctrl+Alt+Shift+/", { selector: "span" })).toBeInTheDocument();
    for (const cap of ["Ctrl", "Alt", "Shift", "/"]) {
      expect(screen.getByText(cap, { selector: "kbd" })).toBeInTheDocument();
    }
    unmount();
    vi.stubGlobal("navigator", { platform: "MacIntel" });
    render(<Kbd keys={["ctrl", "alt", "shift", "/"]} />);
    expect(screen.getByLabelText("Control+Option+Shift+/", { selector: "span" })).toBeInTheDocument();
    for (const cap of ["⌃", "⌥", "⇧", "/"]) {
      expect(screen.getByText(cap, { selector: "kbd" })).toBeInTheDocument();
    }
  });
});
