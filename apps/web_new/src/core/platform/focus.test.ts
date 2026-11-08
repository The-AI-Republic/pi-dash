// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Focus-vs-visibility regression (PIDESKAPP-191): `trackFocus` must
// report window focus, not document visibility. A blur on a still-visible
// window is focus loss and must report `false`; the supervisor's
// `if (focused)` effects then stay idle instead of re-running the full
// agent connect sequence when the user leaves the window.
import { afterEach, describe, expect, it, vi } from "vitest";

import { platform as desktopPlatform } from "./tauri.js";
import { platform as webPlatform } from "./web.js";

function setVisibility(hidden: boolean, hasFocus: boolean): void {
  Object.defineProperty(document, "hidden", { value: hidden, configurable: true });
  vi.spyOn(document, "hasFocus").mockReturnValue(hasFocus);
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function collect(platform: typeof webPlatform | typeof desktopPlatform, script: Array<() => void>): boolean[] {
  const values: boolean[] = [];
  const unsubscribe = platform.onFocusChange((focused) => values.push(focused));
  try {
    for (const step of script) step();
  } finally {
    unsubscribe();
  }
  return values;
}

function fire(target: "window" | "document", type: string): void {
  const event = new Event(type);
  if (target === "window") window.dispatchEvent(event);
  else document.dispatchEvent(event);
}

describe("onFocusChange reports focus, not visibility (PIDESKAPP-191)", () => {
  for (const [name, platform] of [
    ["web", webPlatform],
    ["desktop", desktopPlatform],
  ] as const) {
    it(`${name}: blur on a still-visible window reports false`, () => {
      setVisibility(false, false);
      const values = collect(platform, [() => fire("window", "blur")]);
      expect(values).toEqual([false]);
    });

    it(`${name}: focus on a visible window reports true`, () => {
      setVisibility(false, true);
      const values = collect(platform, [() => fire("window", "focus")]);
      expect(values).toEqual([true]);
    });

    it(`${name}: visibilitychange to hidden reports false even when focused`, () => {
      setVisibility(true, true);
      const values = collect(platform, [() => fire("document", "visibilitychange")]);
      expect(values).toEqual([false]);
    });

    it(`${name}: visibilitychange to visible+focused reports true`, () => {
      setVisibility(false, true);
      const values = collect(platform, [() => fire("document", "visibilitychange")]);
      expect(values).toEqual([true]);
    });

    it(`${name}: blur then focus round-trip reports [false, true]`, () => {
      const values = collect(platform, [
        () => {
          setVisibility(false, false);
          fire("window", "blur");
        },
        () => {
          setVisibility(false, true);
          fire("window", "focus");
        },
      ]);
      expect(values).toEqual([false, true]);
    });

    it(`${name}: focus followed by visibilitychange with the same value reports once`, () => {
      const values = collect(platform, [
        () => {
          setVisibility(false, true);
          fire("window", "focus");
        },
        () => {
          setVisibility(false, true);
          fire("document", "visibilitychange");
        },
      ]);
      expect(values).toEqual([true]);
    });
  }
});
