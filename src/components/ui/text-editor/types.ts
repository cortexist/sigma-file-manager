// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2026 Cortexist, LLC. All rights reserved.

/**
 * How a markdown file is laid out: the rendered page alone, the source alone (which is the
 * plain text editor), or both side by side.
 */
export type TextEditorMarkdownMode = 'read' | 'split' | 'edit';

/** How the two panes of the split layout are arranged. */
/** A plain text file has no preview to split against, so it is read or edited, nothing else. */
export type TextEditorTextMode = 'read' | 'edit';

/** What a rendered view is asked to find and which hit to bring forward. */
export interface FindRequest {
  query: string;
  matchCase: boolean;
  activeIndex: number;
}
