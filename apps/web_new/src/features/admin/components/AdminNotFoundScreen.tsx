// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Console catch-all screen (ADM-011, from parity/specs/admin.md §1.11):
// unmatched god-mode addresses render this outside the guarded layouts,
// for signed-in and signed-out visitors alike, with a single shortcut
// back to general settings. The illustration label, the heading, and
// the shortcut label are pinned by the parity scenario; the surrounding
// copy is fresh.
import { Link } from "@tanstack/react-router";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { GOD_MODE_GENERAL } from "../adminGuards.js";

export const ADMIN_NOT_FOUND_ART = "/illustrations/not-found.svg";
export const ADMIN_NOT_FOUND_TITLE = "Not found — Pi Dash instance admin";

type RouterLinkProps = Parameters<typeof Link>[0];

// General settings (NEWFRONT-507) is not in the route tree yet, so the
// typed `to` cannot name it; the cast carries the literal through the
// same way CrossAppLink reaches out-of-tree paths.
const shortcutProps = { to: GOD_MODE_GENERAL } as RouterLinkProps;

export function AdminNotFoundScreen(): React.ReactElement {
  useDocumentHead({ title: ADMIN_NOT_FOUND_TITLE, robots: "noindex, nofollow" });
  return (
    <main
      data-testid="admin-not-found"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center"
    >
      <img src={ADMIN_NOT_FOUND_ART} alt="404 - Page not found" width={320} height={240} />
      <h1 className="text-h2 text-(--text)">Oops! Something went wrong.</h1>
      <p className="text-body max-w-md text-(--text-muted)">
        That console address does not match anything. The shortcut below returns to the main settings page.
      </p>
      <Link
        {...shortcutProps}
        className="text-body rounded-(--radius-control) bg-(--accent) px-(--space-4) py-(--space-2) text-(--on-accent)"
      >
        Go to general settings
      </Link>
    </main>
  );
}
