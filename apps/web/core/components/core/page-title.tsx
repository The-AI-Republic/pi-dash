/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect } from "react";

import { SITE_TITLE } from "@pi-dash/constants";

type PageHeadTitleProps = {
  title?: string;
  description?: string;
};

export function PageHead(props: PageHeadTitleProps) {
  const { title } = props;

  useEffect(() => {
    if (title) {
      document.title = title ?? SITE_TITLE;
    }
  }, [title]);

  return null;
}
