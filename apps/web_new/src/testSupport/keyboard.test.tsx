// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The shared keyboard/focus helper's own contract (EXP-023): tab reachability,
// the kit focus ring, name resolution, Enter activation, and content-key
// inertness. The export surfaces consume these same assertions.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import {
  VISIBLE_FOCUS_CLASS,
  expectContentKeysInert,
  expectKeyboardReachable,
  isTabReachable,
  pressEnter,
  pressEscape,
  tabTo,
} from "./keyboard.js";

const RING = "focus-visible:outline-solid focus-visible:outline-2";

function Fixture({ onAction }: { onAction: () => void }): React.ReactElement {
  return (
    <section aria-labelledby="fixture-heading">
      <h2 id="fixture-heading">Fixture</h2>
      <button type="button" className={RING} onClick={onAction}>
        First
      </button>
      <button type="button" className={RING} aria-labelledby="fixture-heading" onClick={onAction}>
        Second
      </button>
      <button type="button" className={RING} disabled>
        Disabled
      </button>
      <a href="#target" className={RING}>
        Link
      </a>
      <input aria-label="Text field" className={RING} />
    </section>
  );
}

describe("keyboard helper", () => {
  it("finds every reachable control and rejects a missing focus ring", () => {
    const onAction = vi.fn();
    render(<Fixture onAction={onAction} />);
    const section = screen.getByRole("region", { name: "Fixture" });
    const controls = expectKeyboardReachable(section);
    expect(controls.map((control) => control.tagName)).toEqual(["BUTTON", "BUTTON", "A", "INPUT"]);
    expect(controls[1]).toHaveAttribute("aria-labelledby", "fixture-heading");

    controls[0]!.className = "no-ring";
    expect(() => expectKeyboardReachable(section)).toThrow(/no visible focus ring/);
  });

  it("rejects a control removed from the tab order", () => {
    render(<Fixture onAction={() => undefined} />);
    const section = screen.getByRole("region", { name: "Fixture" });
    const first = screen.getByRole("button", { name: "First" });
    first.setAttribute("tabindex", "-1");
    expect(isTabReachable(first)).toBe(false);
    expect(() => expectKeyboardReachable(section)).toThrow(/not Tab-reachable/);
  });

  it("treats a non-negative tabindex as reachable and a disabled control as unreachable", () => {
    render(
      <div>
        <button type="button" tabIndex={0}>
          Zero
        </button>
        <button type="button" tabIndex={2}>
          Two
        </button>
        <button type="button" disabled>
          Off
        </button>
      </div>
    );
    expect(isTabReachable(screen.getByRole("button", { name: "Zero" }))).toBe(true);
    expect(isTabReachable(screen.getByRole("button", { name: "Two" }))).toBe(true);
    expect(isTabReachable(screen.getByRole("button", { name: "Off" }))).toBe(false);
  });

  it("tabs to a named control, reports the path and fires it on Enter", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(<Fixture onAction={onAction} />);
    const second = screen.getByRole("button", { name: "Fixture" });
    const visited = await tabTo(user, second);
    expect(visited).toEqual(["First"]);
    expect(second).toHaveFocus();
    await pressEnter(user);
    expect(onAction).toHaveBeenCalledTimes(1);
  });

  it("throws when Tab never reaches the requested control", async () => {
    const user = userEvent.setup();
    render(<Fixture onAction={() => undefined} />);
    const disabled = screen.getByRole("button", { name: "Disabled" });
    await expect(tabTo(user, disabled, 2)).rejects.toThrow(/Tab never reached/);
  });

  it("leaves content keys inert outside a text field", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(<Fixture onAction={onAction} />);
    const first = screen.getByRole("button", { name: "First" });
    first.focus();
    await expectContentKeysInert(user, () => first.textContent ?? "");
    expect(onAction).not.toHaveBeenCalled();
    expect(first).toHaveFocus();
  });

  it("dismisses with Escape and falls back to the control's text for a name", async () => {
    const user = userEvent.setup();
    const onEscape = vi.fn();
    render(
      <div
        onKeyDown={(event) => {
          if (event.key === "Escape") onEscape();
        }}
      >
        <button type="button" className={RING}>
          Plain text
        </button>
      </div>
    );
    const plain = screen.getByRole("button", { name: "Plain text" });
    plain.focus();
    await pressEscape(user);
    expect(onEscape).toHaveBeenCalledTimes(1);
  });

  it("falls through to the next naming source when labelledby points nowhere", () => {
    render(
      <div>
        <button type="button" className={RING} aria-labelledby="missing-id" aria-label="Fallback label">
          Ignored text
        </button>
        <button type="button" className={RING} aria-labelledby="missing-id" />
      </div>
    );
    const named = expectKeyboardReachable(
      screen.getByRole("button", { name: "Fallback label" }).parentElement as HTMLElement
    );
    expect(named).toHaveLength(2);
    expect(named[1]?.textContent).toBe("");
  });

  it("treats a control whose text content is null as unnamed", () => {
    render(
      <button type="button" className={RING}>
        Named
      </button>
    );
    const control = screen.getByRole("button", { name: "Named" });
    Object.defineProperty(control, "textContent", { value: null, configurable: true });
    // Named only by its text, which is now null: the name resolves to "".
    expect(() => expectKeyboardReachable(control.parentElement as HTMLElement)).not.toThrow();
    expect(isTabReachable(control)).toBe(true);
  });

  it("exports the kit focus-ring token the surfaces assert", () => {
    expect(VISIBLE_FOCUS_CLASS).toBe("focus-visible:outline-solid");
  });
});
