/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useRef, useState } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useOutsideClickDetector } from "@pi-dash/hooks";

const mocks = vi.hoisted(() => ({ isDesktop: vi.fn(() => true) }));

vi.mock("@/services/agent-runtime", () => ({ isDesktop: mocks.isDesktop }));
vi.mock("@pi-dash/propel/tooltip", () => ({
  Tooltip: ({ children }: { children: React.ReactNode }) => children,
}));
// Like the real modal, the stand-in portals out of the tree that opened it.
vi.mock("@pi-dash/ui", async () => {
  const { createPortal } = await import("react-dom");
  return {
    EModalPosition: { CENTER: "" },
    EModalWidth: { XXXL: "" },
    ModalCore: ({ isOpen, children }: { isOpen: boolean; children: React.ReactNode }) =>
      isOpen ? createPortal(<div role="dialog">{children}</div>, document.body) : null,
  };
});

import { DesktopAboutButton } from "../../core/components/desktop-about-button";

const ABOUT = {
  version: "0.3.3",
  notices: [
    {
      file: "pidash-LICENSE.txt",
      component: "Pi Dash",
      license: "AGPL-3.0-only",
      text: "GNU AFFERO GENERAL PUBLIC LICENSE",
    },
    { file: "agent-engine-LICENSE.txt", component: "Agent engine", license: "Apache-2.0", text: "Apache License" },
    {
      file: "agent-engine-NOTICE.txt",
      component: "Agent engine notice",
      license: "Apache-2.0",
      text: "The upstream binary is renamed to pidash-agent-engine.",
    },
  ],
};

let invoke: ReturnType<typeof vi.fn>;

beforeEach(() => {
  invoke = vi.fn().mockResolvedValue(ABOUT);
  Object.assign(window, { __TAURI__: { core: { invoke } } });
});

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI__");
  mocks.isDesktop.mockReturnValue(true);
});

const flush = () => act(async () => {});

/** The sidebar footer below 768px: a press outside the sidebar collapses it, which unmounts the footer. */
function NarrowSidebarFooter() {
  const [collapsed, setCollapsed] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useOutsideClickDetector(ref, () => setCollapsed(true));
  return <div ref={ref}>{!collapsed && <DesktopAboutButton />}</div>;
}

const openDialog = async () => {
  render(<DesktopAboutButton />);
  fireEvent.click(screen.getByRole("button", { name: "About Pi Dash" }));
  await flush();
  return screen.getByRole("dialog");
};

describe("DesktopAboutButton", () => {
  it("renders nothing outside the desktop app", () => {
    mocks.isDesktop.mockReturnValue(false);
    render(<DesktopAboutButton />);
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("keeps the dialog closed until the button is clicked", () => {
    render(<DesktopAboutButton />);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("shows the version and every bundled licence text", async () => {
    const dialog = await openDialog();
    expect(invoke).toHaveBeenCalledWith("desktop_about");
    expect(dialog.textContent).toContain("Version 0.3.3");
    expect(dialog.textContent).toContain("Pi Dash (AGPL-3.0-only)");
    expect(dialog.textContent).toContain("GNU AFFERO GENERAL PUBLIC LICENSE");
    expect(dialog.textContent).toContain("Agent engine (Apache-2.0)");
    expect(dialog.textContent).toContain("Apache License");
    expect(dialog.textContent).toContain("The upstream binary is renamed to pidash-agent-engine.");
  });

  it("states that the engine binary was renamed even when the notice file is missing", async () => {
    invoke.mockResolvedValue({
      version: "0.3.3",
      notices: [
        { file: "agent-engine-NOTICE.txt", component: "Agent engine notice", license: "Apache-2.0", text: null },
      ],
    });
    const dialog = await openDialog();
    expect(dialog.textContent).toContain("renamed to pidash-agent-engine");
    expect(dialog.textContent).toContain("This build does not include agent-engine-NOTICE.txt.");
  });

  it("reports a failure to load the texts", async () => {
    invoke.mockRejectedValue("resource dir: unknown path");
    const dialog = await openDialog();
    expect(dialog.textContent).toContain("Could not load the licence texts: resource dir: unknown path");
  });

  it("closes from the Close button", async () => {
    await openDialog();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("stays open when pressed inside while the sidebar collapses on outside clicks", async () => {
    render(<NarrowSidebarFooter />);
    fireEvent.click(screen.getByRole("button", { name: "About Pi Dash" }));
    await flush();
    fireEvent.mouseDown(screen.getByText("GNU AFFERO GENERAL PUBLIC LICENSE"));
    expect(screen.queryByRole("dialog")).not.toBeNull();
  });
});
