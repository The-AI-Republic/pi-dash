// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import * as React from "react";
import { Button } from "./Button";
import { createKitToastManager, showToast, ToastHost } from "./Toast";

const manager = createKitToastManager();

export const Notifications = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8 }}>
    <ToastHost manager={manager} />
    <Button
      onClick={() => {
        showToast(manager, { title: "Saved", description: "Issue updated" });
      }}
    >
      Show toast
    </Button>
    <Button
      onClick={() => {
        showToast(manager, {
          title: "Archived",
          description: "Issue moved to the archive",
          actionLabel: "Undo",
          onAction: () => {},
        });
      }}
    >
      Show with action
    </Button>
  </div>
);
