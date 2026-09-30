// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Story smoke coverage: every Ladle story module must load and every named
// story must render without crashing. The Ladle dev server itself cannot
// start in this workspace yet (root pnpm override forces a broken
// mdast-util-to-hast under remark-rehype; see the process comment on
// NEWFRONT-1), so this suite is the executable proof for the stories until
// the server runs.
import { render } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it } from "vitest";
import * as buttonStories from "./components/Button.stories";
import * as fieldsStories from "./components/fields.stories";
import * as iconButtonStories from "./components/IconButton.stories";
import * as menuStories from "./components/Menu.stories";
import * as dialogStories from "./components/Dialog.stories";
import * as overlaysStories from "./components/overlays.stories";
import * as primitivesStories from "./components/primitives.stories";
import * as toastStories from "./components/Toast.stories";
import * as virtualListStories from "./components/VirtualList.stories";

const modules: Record<string, Record<string, unknown>> = {
  "Button.stories": buttonStories,
  "fields.stories": fieldsStories,
  "IconButton.stories": iconButtonStories,
  "Menu.stories": menuStories,
  "Dialog.stories": dialogStories,
  "overlays.stories": overlaysStories,
  "primitives.stories": primitivesStories,
  "Toast.stories": toastStories,
  "VirtualList.stories": virtualListStories,
};

describe("stories", () => {
  for (const [name, stories] of Object.entries(modules)) {
    const exports = Object.entries(stories).filter(([key]) => key !== "default");
    it(`${name} exports at least one story`, () => {
      expect(exports.length).toBeGreaterThan(0);
    });
    for (const [storyName, story] of exports) {
      it(`${name} renders ${storyName}`, () => {
        const Story = story as () => React.ReactElement;
        const { container, unmount } = render(<Story />);
        expect(container.firstElementChild).not.toBeNull();
        unmount();
      });
    }
  }
});
