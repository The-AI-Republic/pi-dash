// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Document statistics (parity/specs/pages.md §6.8, PAGE-049). Four small
// cards over the live document: words, characters, paragraphs and an
// estimate of how long the page takes to read. They follow typing because
// the pane hands them the document the canvas paints.

import * as React from "react";

import { pageReadTimeLabel, type PageDocumentStats as PageStats } from "../lib/pagePane.js";

export interface PageDocumentStatsProps {
  stats: PageStats;
}

interface StatCard {
  key: string;
  label: string;
  value: string;
}

function statCards(stats: PageStats): StatCard[] {
  return [
    { key: "words", label: "Words", value: String(stats.words) },
    { key: "characters", label: "Characters", value: String(stats.characters) },
    { key: "paragraphs", label: "Paragraphs", value: String(stats.paragraphs) },
    { key: "read-time", label: "Read time", value: pageReadTimeLabel(stats.words) },
  ];
}

export function PageDocumentStats({ stats }: PageDocumentStatsProps): React.ReactElement {
  return (
    <div data-testid="page-document-stats" className="grid grid-cols-2 gap-(--space-2)">
      {statCards(stats).map((card) => (
        <div
          key={card.key}
          data-stat={card.key}
          className="rounded-(--radius-card) border border-(--border) px-(--space-3) py-(--space-2)"
        >
          <h6 className="text-h6 text-(--text)">{card.value}</h6>
          <span className="text-caption text-(--text-muted)">{card.label}</span>
        </div>
      ))}
    </div>
  );
}
