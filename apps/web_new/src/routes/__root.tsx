// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { createRootRoute, Outlet } from "@tanstack/react-router";

// Scaffold root route. Area epics replace this with the real shell
// (shared/shell AppShell, workspace layout, god-mode/spaces subtrees).
export const Route = createRootRoute({
  component: function RootComponent() {
    return (
      <main className="pidash-scaffold">
        <h1>Pi Dash</h1>
        <p>New frontend scaffold. Areas land here epic by epic.</p>
        <Outlet />
      </main>
    );
  },
});
