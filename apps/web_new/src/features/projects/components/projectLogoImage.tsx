// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emblem artwork for a project logo box (PSET-005/007). Uploaded marks
// resolve against the API origin the bootstrap client carries; icon marks
// are drawn by the lazy symbol reader. Split from ProjectLogo so the
// shell rail pays for the client import only when a stored mark is
// actually artwork, not on every first paint.
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { ProjectIcon } from "../../../shared/pickers/ProjectIcon.js";
import type { ProjectMark } from "../../../shared/pickers/projectIcon.js";

export interface EmblemArtworkProps {
  mark: Extract<ProjectMark, { kind: "image" | "icon" }>;
  identifier: string;
}

/** The uploaded image or chosen symbol inside a logo box. */
export function EmblemArtwork({ mark, identifier }: EmblemArtworkProps): React.ReactElement {
  if (mark.kind === "icon") {
    return <ProjectIcon mark={mark} identifier={identifier} size={22} testId="project-logo-icon" />;
  }
  const base = getAppClient().baseUrl.replace(/\/+$/, "");
  const src = /^[a-z][a-z\d+.-]*:/i.test(mark.url) ? mark.url : `${base}${mark.url}`;
  return <img key={src} src={src} alt="" className="size-full object-cover" data-testid="project-logo-image" />;
}
