// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { describe, expect, it } from "vitest";
import { tabbableWithin, wrapTabTarget } from "./focusTrap";

function host(html: string): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = html;
  document.body.append(container);
  return container;
}

describe("tabbableWithin", () => {
  it("lists the container's controls in DOM order", () => {
    const container = host(
      '<button id="a">A</button><input id="b" /><a id="c" href="#x">C</a><textarea id="d"></textarea>'
    );
    expect(tabbableWithin(container).map((el) => el.id)).toEqual(["a", "b", "c", "d"]);
  });

  it("skips disabled, removed-from-order, hidden and inert controls", () => {
    const container = host(`
      <button id="keep">Keep</button>
      <button id="disabled" disabled>Off</button>
      <button id="negative" tabindex="-1">Skipped</button>
      <input id="hidden" type="hidden" />
      <div inert><button id="inert">Inert</button></div>
      <button id="last">Last</button>
    `);
    expect(tabbableWithin(container).map((el) => el.id)).toEqual(["keep", "last"]);
  });

  it("drops controls with no visible box, such as collapsed content", () => {
    const container = host('<button id="shown">Shown</button><button id="collapsed">Collapsed</button>');
    const collapsed = container.querySelector<HTMLElement>("#collapsed")!;
    collapsed.checkVisibility = () => false;
    expect(tabbableWithin(container).map((el) => el.id)).toEqual(["shown"]);
  });

  it("returns nothing for a missing container", () => {
    expect(tabbableWithin(null)).toEqual([]);
  });
});

describe("wrapTabTarget", () => {
  it("wraps a forward Tab on the last control to the first", () => {
    const container = host('<button id="first">First</button><button id="last">Last</button>');
    const last = container.querySelector<HTMLElement>("#last")!;
    expect(wrapTabTarget(container, last, false)?.id).toBe("first");
  });

  it("wraps a shift-Tab on the first control to the last", () => {
    const container = host('<button id="first">First</button><button id="last">Last</button>');
    const first = container.querySelector<HTMLElement>("#first")!;
    expect(wrapTabTarget(container, first, true)?.id).toBe("last");
  });

  it("leaves the browser's own move alone inside the container", () => {
    const container = host('<button id="first">First</button><button id="last">Last</button>');
    const first = container.querySelector<HTMLElement>("#first")!;
    expect(wrapTabTarget(container, first, false)).toBeNull();
  });

  it("leaves focus alone when it sits outside the container", () => {
    const container = host('<button id="first">First</button><button id="last">Last</button>');
    const outside = document.createElement("button");
    document.body.append(outside);
    expect(wrapTabTarget(container, outside, false)).toBeNull();
    expect(wrapTabTarget(container, outside, true)).toBeNull();
  });

  it("does nothing without focus, a container, or stops", () => {
    const container = host('<button id="first">First</button>');
    // A caret on the container itself moves to the first stop on its own.
    expect(wrapTabTarget(container, container, false)).toBeNull();
    expect(wrapTabTarget(container, null, false)).toBeNull();
    const bare = host("<p>bare</p>");
    expect(wrapTabTarget(bare, bare.firstElementChild, false)).toBeNull();
    expect(wrapTabTarget(host("<p>bare</p>"), container.querySelector("#first"), false)).toBeNull();
    expect(wrapTabTarget(null, container.querySelector("#first"), false)).toBeNull();
  });

  it("leaves a forward Tab that is not on the last stop alone", () => {
    const container = host('<button id="first">First</button><button id="last">Last</button>');
    const first = container.querySelector<HTMLElement>("#first")!;
    expect(wrapTabTarget(container, first, false)).toBeNull();
  });

  it("leaves a shift-Tab that is not on the first stop alone", () => {
    const container = host('<button id="first">First</button><button id="last">Last</button>');
    const last = container.querySelector<HTMLElement>("#last")!;
    expect(wrapTabTarget(container, last, true)).toBeNull();
  });
});
