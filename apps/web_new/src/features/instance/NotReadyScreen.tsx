// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Full-screen not-ready state (parity/specs/workspace-settings.md §35,
// WSET-035). The instance answered but setup is unfinished, so no
// workspace content or navigation may render. The single action leads
// an administrator to the setup entry.

import * as React from "react";

import { useDocumentHead } from "../../shared/shell/index.js";
import { ADMIN_ENTRY_HREF, PRODUCT_MARK_SRC, PRODUCT_NAME } from "./brand.js";

export const NOT_READY_HEADING = "Welcome to Pi Dash";
export const NOT_READY_BODY =
  "Set up your instance and create your first workspace to begin managing projects and work.";

export function NotReadyScreen(): React.ReactElement {
  useDocumentHead({ title: `${PRODUCT_NAME} - Setup`, robots: "noindex, nofollow" });
  return (
    <main
      data-testid="instance-not-ready"
      className="relative flex min-h-screen flex-col items-center justify-center overflow-hidden bg-(--bg) px-(--space-6) py-(--space-12) text-center text-(--text)"
    >
      <img
        src={PRODUCT_MARK_SRC}
        alt=""
        aria-hidden="true"
        width={64}
        height={64}
        className="pointer-events-none absolute -top-16 -left-16 size-48 opacity-10"
      />
      <img
        src={PRODUCT_MARK_SRC}
        alt=""
        aria-hidden="true"
        width={64}
        height={64}
        className="pointer-events-none absolute -right-14 -bottom-12 size-44 opacity-10"
      />
      <div className="relative flex max-w-lg flex-col items-center gap-(--space-6)">
        <img src={PRODUCT_MARK_SRC} alt="Pi Dash mark" width={96} height={96} className="size-24" />
        <div className="flex flex-col items-center gap-(--space-3)">
          <h1 className="text-h2">{NOT_READY_HEADING}</h1>
          <p className="text-body text-(--text-muted)">{NOT_READY_BODY}</p>
        </div>
        <a
          href={ADMIN_ENTRY_HREF}
          className="text-body inline-flex min-w-64 items-center justify-center rounded-(--radius-control) bg-(--accent) px-(--space-6) py-(--space-3) font-medium text-(--on-accent) hover:opacity-90"
        >
          Get started
        </a>
      </div>
    </main>
  );
}
