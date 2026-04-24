/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { USER_TRACKER_ELEMENTS } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
// ui
import { getButtonStyling } from "@pi-dash/propel/button";
import { PiDashLogo } from "@pi-dash/propel/icons";
// helpers
import { cn } from "@pi-dash/utils";

export function ProductUpdatesFooter() {
  const { t } = useTranslation();
  return (
    <div className="m-6 mb-4 flex flex-shrink-0 items-center justify-between gap-4">
      <div className="flex items-center gap-2">
        <a
          href="https://github.com/The-AI-Republic/pi-dash#readme"
          target="_blank"
          className="text-13 text-secondary underline-offset-1 outline-none hover:text-primary hover:underline"
          rel="noreferrer"
        >
          {t("docs")}
        </a>
        <svg viewBox="0 0 2 2" className="h-0.5 w-0.5 fill-current">
          <circle cx={1} cy={1} r={1} />
        </svg>
        <a
          data-ph-element={USER_TRACKER_ELEMENTS.CHANGELOG_REDIRECTED}
          href="https://github.com/The-AI-Republic/pi-dash#readme"
          target="_blank"
          className="text-13 text-secondary underline-offset-1 outline-none hover:text-primary hover:underline"
          rel="noreferrer"
        >
          {t("full_changelog")}
        </a>
        <svg viewBox="0 0 2 2" className="h-0.5 w-0.5 fill-current">
          <circle cx={1} cy={1} r={1} />
        </svg>
        <a
          href="mailto:support@airepublic.com"
          target="_blank"
          className="text-13 text-secondary underline-offset-1 outline-none hover:text-primary hover:underline"
          rel="noreferrer"
        >
          {t("support")}
        </a>
        <svg viewBox="0 0 2 2" className="h-0.5 w-0.5 fill-current">
          <circle cx={1} cy={1} r={1} />
        </svg>
        <a
          href="https://github.com/The-AI-Republic/pi-dash/discussions"
          target="_blank"
          className="text-13 text-secondary underline-offset-1 outline-none hover:text-primary hover:underline"
          rel="noreferrer"
        >
          Forum
        </a>
      </div>
      <a
        href="https://airepublic.com/pages"
        target="_blank"
        className={cn(
          getButtonStyling("secondary", "base"),
          "flex items-center gap-1.5 text-center font-medium underline-offset-2 outline-none hover:underline"
        )}
        rel="noreferrer"
      >
        <PiDashLogo className="h-4 w-auto text-primary" />
        {t("powered_by_pi_dash_pages")}
      </a>
    </div>
  );
}
