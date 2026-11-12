// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Repository star link (shell spec, SHELL-103). Opens the project's
// repository in a new tab with safe link attributes; the icon swaps
// between the white and black star assets with the resolved theme so
// it stays legible on dark and light chrome alike. The artwork is
// original to this app (a five-point star drawn for it), served from
// the public logos directory.

import * as React from "react";

import { useResolvedScheme } from "../../core/theme/ThemeProvider.js";

/** The project's public repository. */
export const REPOSITORY_URL = "https://github.com/The-AI-Republic/pi-dash";

const STAR_WHITE_SRC = "/logos/github-white.png";
const STAR_BLACK_SRC = "/logos/github-black.png";

/** The star asset for a resolved document scheme (dark bases take white). */
export function starIconSrc(scheme: string, customDark = false): string {
  if (scheme === "dark" || scheme === "dark-contrast") return STAR_WHITE_SRC;
  if (scheme === "custom" && customDark) return STAR_WHITE_SRC;
  return STAR_BLACK_SRC;
}

export function RepoStar(): React.ReactElement {
  const theme = useResolvedScheme();
  return (
    <a href={REPOSITORY_URL} target="_blank" rel="noopener noreferrer" aria-label="Star us on GitHub">
      <img src={starIconSrc(theme.scheme, theme.customDark)} alt="GitHub" width={16} height={16} className="size-4" />
    </a>
  );
}
