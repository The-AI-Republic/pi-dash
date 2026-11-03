// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The browser entry is thin but still decides when the app mounts: it
// restores the cache before creating the router and renders the composed
// AppRoot inside StrictMode. The behavior below that stack is covered in
// features/instance/entry.test.tsx; this suite holds the bootstrap order.
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const rendered = vi.hoisted(() => ({ elements: [] as React.ReactElement[] }));

vi.mock("react-dom/client", () => ({
  createRoot: () => ({
    render: (element: React.ReactElement) => {
      rendered.elements.push(element);
    },
  }),
}));

// The generated tree pulls every route module into the import, which took
// the mount test to ~24 s focused against the 30 s full-run budget
// (NEWFRONT-861). This suite asserts the bootstrap structure, not the route
// table, so it boots a bare root route; createRouter itself stays real.
vi.mock("./routeTree.gen", async () => {
  const { createRootRoute } = await import("@tanstack/react-router");
  return { routeTree: createRootRoute() };
});

beforeEach(() => {
  rendered.elements.length = 0;
  vi.resetModules();
  document.body.innerHTML = "";
});

afterEach(() => {
  document.body.innerHTML = "";
});

describe("main entry", () => {
  it("mounts the composed app root on the #root element", async () => {
    document.body.innerHTML = '<div id="root"></div>';
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    await import("./main.js");
    expect(rendered.elements).toHaveLength(1);
    const element = rendered.elements[0] as React.ReactElement<{ children: React.ReactElement<{ router: unknown }> }>;
    expect(element.type).toBe(React.StrictMode);
    const appRoot = element.props.children;
    expect(typeof appRoot.type).toBe("function");
    expect(typeof appRoot.props.router).toBe("object");
    info.mockRestore();
  });

  it("refuses to start without a #root element", async () => {
    await expect(import("./main.js")).rejects.toThrow("Missing #root element");
    expect(rendered.elements).toHaveLength(0);
  });
});
