// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
import { Plus, X } from "lucide-react";
import { IconButton } from "./IconButton";

export const Variants = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8 }}>
    <IconButton label="Close">
      <X size={16} strokeWidth={1.5} />
    </IconButton>
    <IconButton label="Add" variant="primary">
      <Plus size={16} strokeWidth={1.5} />
    </IconButton>
    <IconButton label="Add" variant="secondary">
      <Plus size={16} strokeWidth={1.5} />
    </IconButton>
  </div>
);

export const Sizes = (): React.ReactElement => (
  <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
    <IconButton label="Close" size="small">
      <X size={16} strokeWidth={1.5} />
    </IconButton>
    <IconButton label="Close" size="medium">
      <X size={16} strokeWidth={1.5} />
    </IconButton>
    <IconButton label="Close" size="large">
      <X size={16} strokeWidth={1.5} />
    </IconButton>
  </div>
);
