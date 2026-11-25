/** Types for the help system (docs/design/ux-help-system.md §4). */

export type HelpAudience = 'beginner' | 'advanced' | 'both';

export interface HelpTopicMeta {
  slug: string;
  title: string;
  summary: string;
  audience: HelpAudience;
  /** Estimated reading time in minutes. */
  minutes: number;
  related: string[];
}

export interface HelpIndex {
  version: number;
  topics: HelpTopicMeta[];
}

/** Raw markdown + metadata for one topic (GET /api/help/:slug or bundled .md). */
export interface HelpTopic extends HelpTopicMeta {
  markdown: string;
}

export interface HelpTipContent {
  /** One-sentence "what is this". */
  what: string;
  /** One-sentence "why it matters / what it costs". */
  why: string;
  /** Help Center topic slug this tip links to. */
  topic: string;
  /** Optional glossary term to render inline. */
  term?: string;
  /** Which mode the tip is written for; defaults to both. */
  audience?: HelpAudience;
}

export interface HelpTocEntry {
  id: string;
  text: string;
  level: number;
}

export interface HelpRoute {
  slug: string | null;
  anchor: string | null;
}
