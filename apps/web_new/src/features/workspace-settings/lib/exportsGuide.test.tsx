// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, renderHook, screen } from "@testing-library/react";
import * as React from "react";
import { afterEach, describe, expect, it } from "vitest";

import {
  getWorkspaceExportsGuide,
  registerWorkspaceExportsGuide,
  resetWorkspaceExportsGuide,
  useWorkspaceExportsGuide,
} from "./exportsGuide.js";

function Guide({ workspaceSlug }: { workspaceSlug: string }): React.ReactElement {
  return <p>guide of {workspaceSlug}</p>;
}

function OtherGuide(): React.ReactElement {
  return <p>other guide</p>;
}

afterEach(() => {
  resetWorkspaceExportsGuide();
});

describe("exports guide seam", () => {
  it("starts empty", () => {
    expect(getWorkspaceExportsGuide()).toBeNull();
  });

  it("registers the exports area's component and hands it the workspace", () => {
    registerWorkspaceExportsGuide(Guide);
    render(<Guide workspaceSlug="acme" />);
    expect(screen.getByText("guide of acme")).toBeInTheDocument();
  });

  it("unregisters only its own registration", () => {
    const first = registerWorkspaceExportsGuide(Guide);
    const second = registerWorkspaceExportsGuide(OtherGuide);
    first();
    expect(getWorkspaceExportsGuide()).toBe(OtherGuide);
    second();
    expect(getWorkspaceExportsGuide()).toBeNull();
  });

  it("notifies consumers as the guide mounts and leaves", () => {
    const { result } = renderHook(() => useWorkspaceExportsGuide());
    expect(result.current).toBeNull();
    let unregister = (): void => undefined;
    act(() => {
      unregister = registerWorkspaceExportsGuide(Guide);
    });
    expect(result.current).toBe(Guide);
    act(() => {
      unregister();
    });
    expect(result.current).toBeNull();
  });
});
