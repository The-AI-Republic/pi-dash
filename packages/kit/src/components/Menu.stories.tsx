// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Button } from "./Button";
import { Menu } from "./Menu";

export const Actions = (): React.ReactElement => (
  <Menu
    trigger={<Button>Actions</Button>}
    items={[
      { label: "Edit", onSelect: () => {} },
      { label: "Duplicate", onSelect: () => {} },
      { separator: true },
      { label: "Archive", onSelect: () => {}, disabled: true },
    ]}
  />
);
