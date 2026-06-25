/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Outlet } from "react-router";
import { AppHeader } from "@/components/core/app-header";
import { ContentWrapper } from "@/components/core/content-wrapper";
import { GlobalViewIdProvider } from "@/hooks/use-global-view-id";
import { GlobalIssuesHeader } from "../workspace-views/header";

// The `/all-issues` route pins the static "all-issues" view so the shared
// global-issues header and layout root behave exactly like
// `/workspace-views/all-issues` without relying on a `:globalViewId` param.
export default function AllIssuesLayout() {
  return (
    <GlobalViewIdProvider value="all-issues">
      <AppHeader header={<GlobalIssuesHeader />} />
      <ContentWrapper>
        <Outlet />
      </ContentWrapper>
    </GlobalViewIdProvider>
  );
}
