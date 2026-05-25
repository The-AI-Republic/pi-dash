/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Link, Outlet, useLocation, useParams } from "react-router";
import { CalendarClock, List as ListIcon } from "lucide-react";
import { useTranslation } from "@pi-dash/i18n";
import { cn } from "@pi-dash/utils";
import { AppHeader } from "@/components/core/app-header";
import { ContentWrapper } from "@/components/core/content-wrapper";
import { ProjectSchedulersHeader } from "./header";

export default function ProjectSchedulersLayout() {
  const { workspaceSlug, projectId } = useParams<{ workspaceSlug: string; projectId: string }>();
  const { pathname } = useLocation();
  const { t } = useTranslation();

  const basePath = `/${workspaceSlug}/projects/${projectId}/schedulers`;
  const isCalendar = pathname.endsWith("/calendar");

  return (
    <>
      <AppHeader header={<ProjectSchedulersHeader />} />
      <ContentWrapper>
        <div className="flex h-full w-full flex-col">
          {/* Tab bar: List | Calendar. Single sidebar entry, internal toggle. */}
          <div className="flex items-center gap-1 border-b border-subtle px-4 pt-3">
            <SchedulerTabLink to={basePath} active={!isCalendar} icon={<ListIcon className="size-4" />}>
              {t("scheduler_bindings.tabs.list")}
            </SchedulerTabLink>
            <SchedulerTabLink
              to={`${basePath}/calendar`}
              active={isCalendar}
              icon={<CalendarClock className="size-4" />}
            >
              {t("scheduler_bindings.tabs.calendar")}
            </SchedulerTabLink>
          </div>
          <div className="flex-1 overflow-auto">
            <Outlet />
          </div>
        </div>
      </ContentWrapper>
    </>
  );
}

type SchedulerTabLinkProps = {
  to: string;
  active: boolean;
  icon: React.ReactNode;
  children: React.ReactNode;
};

function SchedulerTabLink({ to, active, icon, children }: SchedulerTabLinkProps) {
  return (
    <Link
      to={to}
      className={cn(
        "inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-13 font-medium transition-colors",
        active
          ? "border-primary text-primary"
          : "border-transparent text-secondary hover:border-subtle hover:text-primary"
      )}
    >
      {icon}
      {children}
    </Link>
  );
}
