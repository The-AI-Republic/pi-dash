// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project mark tile (parity/specs/issues.md §17, ISS-211). An emoji, the
// project's chosen icon in its own color, or the short code's initial, in
// the tile size each caller asks for. Decorative: the row label already
// names the project for assistive tech.
import * as React from "react";

import { projectMark, projectMarkText } from "./projectIcon.js";
import { ProjectIcon } from "./ProjectIcon.js";

/** Resolve a stored emblem path to something an <img> can load. */
export function resolveMarkSource(url: string, apiBaseUrl: string | undefined): string {
  if (/^[a-z][a-z\d+.-]*:/i.test(url) || url.startsWith("//")) return url;
  if (apiBaseUrl === undefined || apiBaseUrl === "") return url;
  return `${apiBaseUrl.replace(/\/+$/, "")}${url.startsWith("/") ? "" : "/"}${url}`;
}

export interface ProjectMarkProps {
  /** Raw logo value from the project row; parsed defensively. */
  logoProps: Record<string, unknown>;
  /** Short code; its initial is the fallback mark. */
  identifier: string;
  /** Tile edge in pixels. */
  size?: number;
  className?: string;
  /** Base URL that relative uploaded-emblem paths resolve against. */
  apiBaseUrl?: string | undefined;
}

export function ProjectMark({
  logoProps,
  identifier,
  size = 20,
  className,
  apiBaseUrl,
}: ProjectMarkProps): React.ReactElement {
  const mark = projectMark(logoProps);
  const fontSize = size <= 18 ? "text-meta" : "text-body";
  return (
    <span
      data-testid="project-mark"
      data-mark={mark.kind}
      aria-hidden="true"
      className={`flex shrink-0 items-center justify-center overflow-hidden rounded-(--radius-control) bg-(--subtle) ${fontSize} font-medium text-(--text-muted) ${className ?? ""}`}
      style={{ width: size, height: size }}
    >
      {mark.kind === "image" ? (
        <img
          key={mark.url}
          src={resolveMarkSource(mark.url, apiBaseUrl)}
          alt=""
          className="size-full object-cover"
          data-testid="project-mark-image"
        />
      ) : mark.kind === "icon" ? (
        <ProjectIcon mark={mark} identifier={identifier} size={Math.round(size * 0.7)} />
      ) : (
        <span aria-hidden="true">{projectMarkText(mark, identifier)}</span>
      )}
    </span>
  );
}
