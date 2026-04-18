/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { observer } from "mobx-react";
import { useTranslation } from "@apple-pi-dash/i18n";
// ui
import { CycleIcon } from "@apple-pi-dash/propel/icons";
import { Breadcrumbs, Header } from "@apple-pi-dash/ui";
// components
import { BreadcrumbLink } from "@/components/common/breadcrumb-link";
// apple pi dash web components
import { UpgradeBadge } from "@/apple-pi-dash-web/components/workspace/upgrade-badge";

export const WorkspaceActiveCycleHeader = observer(function WorkspaceActiveCycleHeader() {
  const { t } = useTranslation();
  return (
    <Header>
      <Header.LeftItem>
        <Breadcrumbs>
          <Breadcrumbs.Item
            component={
              <BreadcrumbLink
                label={t("active_cycles")}
                icon={<CycleIcon className="h-4 w-4 rotate-180 text-tertiary" />}
              />
            }
          />
        </Breadcrumbs>
        <UpgradeBadge size="md" />
      </Header.LeftItem>
    </Header>
  );
});
