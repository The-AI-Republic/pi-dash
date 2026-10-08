// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
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
