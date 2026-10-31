// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { Button } from "./Button";
import { Dialog } from "./Dialog";

export const Standard = (): React.ReactElement => (
  <Dialog
    trigger={<Button>Edit title</Button>}
    title="Edit title"
    description="Shown in the issue list and search"
    actions={
      <>
        <Button>Cancel</Button>
        <Button variant="primary">Save</Button>
      </>
    }
  >
    <p style={{ margin: 0 }}>Dialog body content goes here.</p>
  </Dialog>
);
