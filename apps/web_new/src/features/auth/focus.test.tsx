// @vitest-environment jsdom
// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { authStepInputProps, pointerOnlyControlProps, swallowEnterInTextField, useFocusOnMount } from "./focus.js";

describe("auth focus conventions", () => {
  it("autofocuses step inputs", () => {
    expect(authStepInputProps()).toEqual({ autoFocus: true });
  });

  it("keeps pointer-only controls out of tab order", () => {
    expect(pointerOnlyControlProps()).toEqual({ tabIndex: -1 });
  });

  it("swallows Enter inside text and email fields", () => {
    const text = document.createElement("input");
    text.type = "text";
    expect(swallowEnterInTextField({ key: "Enter", target: text })).toBe(true);
    const email = document.createElement("input");
    email.type = "email";
    expect(swallowEnterInTextField({ key: "Enter", target: email })).toBe(true);
  });

  it("leaves other keys and controls alone", () => {
    const text = document.createElement("input");
    text.type = "text";
    expect(swallowEnterInTextField({ key: " ", target: text })).toBe(false);
    const box = document.createElement("input");
    box.type = "checkbox";
    expect(swallowEnterInTextField({ key: "Enter", target: box })).toBe(false);
    const area = document.createElement("textarea");
    expect(swallowEnterInTextField({ key: "Enter", target: area })).toBe(false);
    expect(swallowEnterInTextField({ key: "Enter", target: null })).toBe(false);
  });

  it("swallows nothing where the DOM is absent", () => {
    vi.stubGlobal("HTMLInputElement", undefined);
    try {
      expect(swallowEnterInTextField({ key: "Enter", target: {} })).toBe(false);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("focuses links on mount and detaches cleanly", () => {
    function FocusProbe(): React.ReactElement {
      const focusRef = useFocusOnMount<HTMLAnchorElement>();
      return (
        <a href="/invitations" ref={focusRef}>
          View pending invitations
        </a>
      );
    }
    const { unmount } = render(<FocusProbe />);
    expect(screen.getByRole("link", { name: "View pending invitations" })).toHaveFocus();
    unmount();
  });
});
