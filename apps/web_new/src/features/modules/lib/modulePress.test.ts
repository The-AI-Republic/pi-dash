// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Press routing for rows and cards (parity/specs/modules.md §3.5, §3.6,
// MOD-012, MOD-020): a control keeps its own press, plain body content
// belongs to the surface.

// @vitest-environment jsdom
import type * as React from "react";
import { describe, expect, it } from "vitest";

import { modulePressHitControl } from "./modulePress.js";

function press(target: Element | null): React.SyntheticEvent<HTMLElement> {
  return { target } as unknown as React.SyntheticEvent<HTMLElement>;
}

describe("modulePressHitControl", () => {
  it("recognises buttons, anchors and role buttons, even when nested", () => {
    const host = document.createElement("div");
    host.innerHTML =
      '<button id="control"><span id="in-button"></span></button>' +
      '<a id="link"><span id="in-link"></span></a>' +
      '<span id="role" role="button"></span>';
    for (const id of ["control", "in-button", "link", "in-link", "role"]) {
      expect(modulePressHitControl(press(host.querySelector(`#${id}`)))).toBe(true);
    }
  });

  it("treats plain body content as the surface's own press", () => {
    const host = document.createElement("div");
    host.innerHTML = '<p id="body"><span id="text"></span></p>';
    expect(modulePressHitControl(press(host.querySelector("#text")))).toBe(false);
  });

  it("treats a missing target as the body", () => {
    expect(modulePressHitControl(press(null))).toBe(false);
  });
});
