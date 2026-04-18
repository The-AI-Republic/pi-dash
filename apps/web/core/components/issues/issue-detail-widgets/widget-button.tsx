/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import React from "react";
// helpers
import { Button } from "@pi-dash/propel/button";

type Props = {
  icon: React.ReactNode;
  title: string;
  disabled?: boolean;
};

export function IssueDetailWidgetButton(props: Props) {
  const { icon, title, disabled = false } = props;
  return (
    <Button variant={"secondary"} disabled={disabled} size="lg">
      {icon && icon}
      <span className="text-body-xs-medium">{title}</span>
    </Button>
  );
}
