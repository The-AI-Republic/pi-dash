// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Full-screen maintenance state (parity/specs/workspace-settings.md §34,
// WSET-034). The instance-info read failed before the app could start;
// this replaces every route, including god-mode. The illustration swaps
// with the resolved color scheme, the message explains the container is
// likely still starting, and support is reachable without exposing any
// workspace chrome underneath.

import * as React from "react";

import { useResolvedScheme } from "../../core/theme/index.js";
import { useDocumentHead } from "../../shared/shell/index.js";

export const MAINTENANCE_ARM_LIGHT = "/illustrations/maintenance-mode-light.svg";
export const MAINTENANCE_ARM_DARK = "/illustrations/maintenance-mode-dark.svg";
export const MAINTENANCE_HEADING = "Pi Dash didn't start up correctly";
export const MAINTENANCE_SUPPORT_HREF = "mailto:support@airepublic.com";

/** The artwork for a resolved scheme; custom schemes follow their dark half. */
export function maintenanceIllustration(dark: boolean): string {
  return dark ? MAINTENANCE_ARM_DARK : MAINTENANCE_ARM_LIGHT;
}

export function MaintenanceScreen(): React.ReactElement {
  const { scheme, customDark } = useResolvedScheme();
  const dark = scheme === "dark" || scheme === "dark-contrast" || (scheme === "custom" && customDark);
  useDocumentHead({ title: "Pi Dash - Maintenance", robots: "noindex, nofollow" });
  return (
    <main
      data-testid="instance-maintenance"
      className="flex min-h-screen flex-col items-center justify-center gap-(--space-6) bg-(--bg) px-(--space-6) py-(--space-12) text-center text-(--text)"
    >
      <img
        src={maintenanceIllustration(dark)}
        alt=""
        width={288}
        height={176}
        className="h-44 w-72"
        data-maintenance-illustration
      />
      <div className="flex max-w-lg flex-col items-center gap-(--space-3)">
        <h1 className="text-h2">{MAINTENANCE_HEADING}</h1>
        <p className="text-body text-(--text-muted)">
          The services may not have finished starting. Check the container logs for errors, then reload once they are
          running.
        </p>
      </div>
      <a
        href={MAINTENANCE_SUPPORT_HREF}
        className="text-body rounded-(--radius-control) border border-(--border) px-(--space-4) py-(--space-2) hover:bg-(--subtle)"
      >
        Contact Support
      </a>
    </main>
  );
}
