// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Assets tab (parity/specs/pages.md §6.7, PAGE-048). The pictures the
// document embeds, in document order: a thumbnail, the name and a download
// action that appears on hover. Selecting a row opens the picture; plain
// links are never rows. With no attachments the tab explains the empty
// state instead of showing a bare list.

import { EmptyState } from "@pidash/kit";
import { Download } from "lucide-react";
import * as React from "react";

import type { PageAssetRow } from "../lib/pagePane.js";

export const PAGE_ASSETS_EMPTY_TITLE = "Missing images";
export const PAGE_ASSETS_EMPTY_BODY = "Pictures added to this page will be listed here.";
export const PAGE_ASSETS_EMPTY_ART_ALT = "depicts the assets of a page";
export const PAGE_ASSETS_DOWNLOAD_LABEL = "Download";

export interface PageAssetsPanelProps {
  assets: readonly PageAssetRow[];
  /**
   * Edition seam for attachment kinds the community build does not render
   * itself (§6.7, §9.4); absent means only the name shows.
   */
  renderAttachment?: ((asset: PageAssetRow) => React.ReactNode) | undefined;
}

function PageAssetsEmpty(): React.ReactElement {
  return (
    <div data-testid="page-assets-empty">
      <EmptyState
        variant="detailed"
        size="small"
        art={
          <img
            src="/empty-state/general/empty-light.svg"
            alt={PAGE_ASSETS_EMPTY_ART_ALT}
            className="h-32 w-32"
            draggable={false}
          />
        }
        title={PAGE_ASSETS_EMPTY_TITLE}
        description={PAGE_ASSETS_EMPTY_BODY}
      />
    </div>
  );
}

function PageAssetRowView({ asset }: { asset: PageAssetRow }): React.ReactElement {
  return (
    <a
      href={asset.viewHref}
      target="_blank"
      rel="noreferrer"
      data-testid="page-asset-row"
      className="group/asset flex items-center gap-(--space-2) rounded-(--radius-control) p-(--space-1) hover:bg-(--subtle)"
    >
      {asset.thumbnailHref === null ? null : (
        <div
          aria-hidden="true"
          style={{ backgroundImage: `url(${JSON.stringify(asset.thumbnailHref)})` }}
          className="size-8 shrink-0 rounded-(--radius-control) border border-(--border) bg-cover bg-center"
        />
      )}
      <p className="text-body min-w-0 flex-1 truncate text-(--text)">{asset.name}</p>
      <a
        href={asset.downloadHref}
        target="_blank"
        rel="noreferrer"
        aria-label={`${PAGE_ASSETS_DOWNLOAD_LABEL} ${asset.name}`}
        className="pointer-events-none inline-flex size-6 shrink-0 items-center justify-center rounded-(--radius-control) text-(--text-muted) opacity-0 group-hover/asset:pointer-events-auto group-hover/asset:opacity-100 hover:bg-(--subtle) hover:text-(--text) focus-visible:pointer-events-auto focus-visible:opacity-100"
      >
        <Download size={14} strokeWidth={1.5} aria-hidden="true" />
      </a>
    </a>
  );
}

export function PageAssetsPanel({ assets, renderAttachment }: PageAssetsPanelProps): React.ReactElement {
  if (assets.length === 0) return <PageAssetsEmpty />;
  return (
    <ul data-testid="page-assets" className="flex flex-col gap-(--space-1)">
      {assets.map((asset) => (
        <li key={`${asset.source}-${asset.name}`} className="min-w-0">
          {asset.kind === "image" ? (
            <PageAssetRowView asset={asset} />
          ) : (
            <div className="flex items-center justify-between gap-(--space-2) p-(--space-1)">
              <p className="text-body min-w-0 flex-1 truncate text-(--text)">{asset.name}</p>
              {renderAttachment?.(asset) ?? null}
            </div>
          )}
        </li>
      ))}
    </ul>
  );
}
