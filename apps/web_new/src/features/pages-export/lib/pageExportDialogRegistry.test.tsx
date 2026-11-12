// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page-export dialog seam (parity/specs/exports.md §0.3): registration,
// cleanup and consumer notification for the Pages detail's dialog slot.
import { act, render, renderHook, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import {
  getPageExportDialog,
  registerPageExportDialog,
  resetPageExportDialog,
  usePageExportDialog,
  type PageExportDialogComponent,
  type PageExportHost,
} from "./pageExportDialogRegistry.js";

const HOST: PageExportHost = {
  pageTitle: "Export me",
  projectId: "project-1",
  workspaceSlug: "acme",
  getHtml: () => "<p>body</p>",
  getMarkdown: () => "body",
};

const Dialog: PageExportDialogComponent = ({ host }) => <p>dialog for {host.pageTitle}</p>;

const OtherDialog: PageExportDialogComponent = () => <p>other dialog</p>;

afterEach(() => {
  resetPageExportDialog();
});

describe("page export dialog seam", () => {
  it("starts empty", () => {
    expect(getPageExportDialog()).toBeNull();
  });

  it("registers the exports area's dialog and hands it the host", () => {
    registerPageExportDialog(Dialog);
    expect(getPageExportDialog()).toBe(Dialog);
    render(<Dialog host={HOST} open onClose={() => undefined} />);
    expect(screen.getByText("dialog for Export me")).toBeInTheDocument();
  });

  it("unregisters only its own registration", () => {
    const first = registerPageExportDialog(Dialog);
    const second = registerPageExportDialog(OtherDialog);
    first();
    expect(getPageExportDialog()).toBe(OtherDialog);
    second();
    expect(getPageExportDialog()).toBeNull();
  });

  it("notifies consumers as the dialog mounts and leaves", () => {
    const { result } = renderHook(() => usePageExportDialog());
    expect(result.current).toBeNull();
    let unregister = (): void => undefined;
    act(() => {
      unregister = registerPageExportDialog(Dialog);
    });
    expect(result.current).toBe(Dialog);
    act(() => {
      unregister();
    });
    expect(result.current).toBeNull();
  });
});
