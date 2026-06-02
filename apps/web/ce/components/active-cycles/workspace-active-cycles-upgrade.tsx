/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { AlertOctagon, BarChart4, CircleDashed, Folder, Microscope } from "lucide-react";
// pi dash imports
import { MARKETING_PRICING_PAGE_LINK } from "@pi-dash/constants";
import { useTranslation } from "@pi-dash/i18n";
import { getButtonStyling } from "@pi-dash/propel/button";
import { SearchIcon } from "@pi-dash/propel/icons";
import { ContentWrapper } from "@pi-dash/ui";
import { cn } from "@pi-dash/utils";
// assets
import ctaL1Dark from "@/app/assets/workspace-active-cycles/cta-l-1-dark.webp?url";
import ctaL1Light from "@/app/assets/workspace-active-cycles/cta-l-1-light.webp?url";
import ctaR1Dark from "@/app/assets/workspace-active-cycles/cta-r-1-dark.webp?url";
import ctaR1Light from "@/app/assets/workspace-active-cycles/cta-r-1-light.webp?url";
import ctaR2Dark from "@/app/assets/workspace-active-cycles/cta-r-2-dark.webp?url";
import ctaR2Light from "@/app/assets/workspace-active-cycles/cta-r-2-light.webp?url";
// components
import { ProIcon } from "@/components/common/pro-icon";
// hooks
import { useUser } from "@/hooks/store/user";

export const WORKSPACE_ACTIVE_CYCLES_DETAILS = [
  {
    key: "10000_feet_view",
    i18n_title: "10,000-feet view of all active cycles.",
    i18n_description:
      "Zoom out to see running cycles across all your projects at once instead of going from Cycle to Cycle in each project.",
    icon: Folder,
  },
  {
    key: "get_snapshot_of_each_active_cycle",
    i18n_title: "Get a snapshot of each active cycle.",
    i18n_description:
      "Track high-level metrics for all active cycles, see their state of progress, and get a sense of scope against deadlines.",
    icon: CircleDashed,
  },
  {
    key: "compare_burndowns",
    i18n_title: "Compare burndowns.",
    i18n_description: "Monitor how each of your teams are performing with a peek into each cycle’s burndown report.",
    icon: BarChart4,
  },
  {
    key: "quickly_see_make_or_break_issues",
    i18n_title: "Quickly see make-or-break work items. ",
    i18n_description:
      "Preview high-priority work items for each cycle against due dates. See all of them per cycle in one click.",
    icon: AlertOctagon,
  },
  {
    key: "zoom_into_cycles_that_need_attention",
    i18n_title: "Zoom into cycles that need attention. ",
    i18n_description: "Investigate the state of any cycle that doesn’t conform to expectations in one click.",
    icon: SearchIcon,
  },
  {
    key: "stay_ahead_of_blockers",
    i18n_title: "Stay ahead of blockers.",
    i18n_description:
      "Spot challenges from one project to another and see inter-cycle dependencies that aren’t obvious from any other view.",
    icon: Microscope,
  },
];

export const WorkspaceActiveCyclesUpgrade = observer(function WorkspaceActiveCyclesUpgrade() {
  const { t } = useTranslation();
  // store hooks
  const {
    userProfile: { data: userProfile },
  } = useUser();

  const isDarkMode = userProfile?.theme.theme === "dark";

  return (
    <ContentWrapper className="gap-10">
      <div
        className={cn("item-center flex min-h-[25rem] justify-between rounded-xl", {
          "bg-gradient-to-l from-[#CFCFCF] to-[#212121]": userProfile?.theme.theme === "dark",
          "bg-gradient-to-l from-[#3b5ec6] to-[#f5f7fe]": userProfile?.theme.theme === "light",
        })}
      >
        <div className="relative flex flex-col justify-center gap-7 px-14 lg:w-1/2">
          <div className="flex max-w-64 flex-col gap-2">
            <h2 className="text-20 font-semibold">{t("On-demand snapshots of all your cycles")}</h2>
            <p className="text-14 font-medium text-tertiary">{t("Monitor cycles across projects, track high-priority work items, and zoom in cycles that need attention.")}</p>
          </div>
          <div className="flex items-center gap-3">
            <a
              className={`${getButtonStyling("primary", "base")} cursor-pointer`}
              href={MARKETING_PRICING_PAGE_LINK}
              target="_blank"
              rel="noreferrer"
            >
              <ProIcon className="h-3.5 w-3.5 text-on-color" />
              {t("Upgrade")}
            </a>
          </div>
          <span className="absolute top-0 left-0">
            <img
              src={isDarkMode ? ctaL1Dark : ctaL1Light}
              className="h-[125px] w-[125px] rounded-xl object-contain"
              alt="l-1"
            />
          </span>
        </div>
        <div className="relative hidden w-1/2 lg:block">
          <span className="absolute right-0 bottom-0">
            <img src={isDarkMode ? ctaR1Dark : ctaR1Light} className="h-full w-full object-contain" alt="r-1" />
          </span>
          <span className="absolute right-1/2 -bottom-16 rounded-xl">
            <img src={isDarkMode ? ctaR2Dark : ctaR2Light} className="h-full w-full object-contain" alt="r-2" />
          </span>
        </div>
      </div>
      <div className="grid h-full grid-cols-1 gap-5 pb-8 lg:grid-cols-2 xl:grid-cols-3">
        {WORKSPACE_ACTIVE_CYCLES_DETAILS.map((item) => (
          <div key={item.i18n_title} className="flex min-h-32 w-full flex-col gap-2 rounded-md bg-layer-1 p-4">
            <div className="flex justify-between gap-2">
              <h3 className="font-medium">{t(item.i18n_title)}</h3>
              <item.icon className="text-blue-500 mt-1 h-4 w-4" />
            </div>
            <span className="text-13 text-tertiary">{t(item.i18n_description)}</span>
          </div>
        ))}
      </div>
    </ContentWrapper>
  );
});
