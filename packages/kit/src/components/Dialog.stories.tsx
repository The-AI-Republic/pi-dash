// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
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
