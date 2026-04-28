/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { Github } from "lucide-react";
import { Breadcrumbs } from "@pi-dash/ui";
import { BreadcrumbLink } from "@/components/common/breadcrumb-link";
import { SettingsPageHeader } from "@/components/settings/page-header";

export function GithubProjectSettingsHeader() {
  return (
    <SettingsPageHeader
      leftItem={
        <div className="flex items-center gap-2">
          <Breadcrumbs>
            <Breadcrumbs.Item
              component={
                <BreadcrumbLink label="GitHub" icon={<Github className="size-4 text-tertiary" />} />
              }
            />
          </Breadcrumbs>
        </div>
      }
    />
  );
}
