/**
 * Copyright (c) 2023-present Apple Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

// apple pi dash imports
import { ScrollArea } from "@apple-pi-dash/propel/scrollarea";
// apple pi dash web imports
import { PageNavigationPaneOutlineTabEmptyState } from "@/apple-pi-dash-web/components/pages/navigation-pane/tab-panels/empty-states/outline";
// store
import type { TPageInstance } from "@/store/pages/base-page";
// local imports
import { PageContentBrowser } from "../../editor/summary";

type Props = {
  page: TPageInstance;
};

export function PageNavigationPaneOutlineTabPanel(props: Props) {
  const { page } = props;
  // derived values
  const {
    editor: { editorRef },
  } = page;

  return (
    <ScrollArea
      orientation="vertical"
      size="sm"
      scrollType="hover"
      className="hide-scrollbar size-full overflow-y-auto"
      viewportClassName="px-4"
    >
      <PageContentBrowser
        className="mt-0"
        editorRef={editorRef}
        emptyState={<PageNavigationPaneOutlineTabEmptyState />}
      />
    </ScrollArea>
  );
}
