// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
