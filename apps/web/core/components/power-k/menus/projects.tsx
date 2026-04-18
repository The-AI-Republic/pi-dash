/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
// components
import { Logo } from "@apple-pi-dash/propel/emoji-icon-picker";
// apple pi dash imports
import type { TPartialProject } from "@/apple-pi-dash-web/types";
// local imports
import { PowerKMenuBuilder } from "./builder";

type Props = {
  projects: TPartialProject[];
  onSelect: (project: TPartialProject) => void;
};

export function PowerKProjectsMenu({ projects, onSelect }: Props) {
  return (
    <PowerKMenuBuilder
      items={projects}
      getKey={(project) => project.id}
      getIconNode={(project) => (
        <span className="shrink-0">
          <Logo logo={project.logo_props} size={14} />
        </span>
      )}
      getValue={(project) => project.name}
      getLabel={(project) => project.name}
      onSelect={onSelect}
      emptyText="No projects found"
    />
  );
}
