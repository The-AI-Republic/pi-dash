/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useTheme } from "next-themes";
// pi dash imports
import { Toast } from "@pi-dash/propel/toast";
import { resolveGeneralTheme } from "@pi-dash/utils";

export function ToastProvider({ children }: { children: React.ReactNode }) {
  // themes
  const { resolvedTheme } = useTheme();

  return (
    <>
      <Toast theme={resolveGeneralTheme(resolvedTheme)} />
      {children}
    </>
  );
}
