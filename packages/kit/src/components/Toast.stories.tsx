// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
