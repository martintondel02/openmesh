/** Help Center data, search, markdown rendering and deep-link helpers. */
import type { HelpAudience, HelpIndex, HelpRoute, HelpTocEntry, HelpTopic, HelpTopicMeta } from '../types/help';

export const HELP_TOPIC_SLUGS = [
  'getting-started', 'install', 'first-mesh', 'radio-and-region', 'messaging', 'media',
  'channels', 'contacts-identity', 'map-and-position', 'nodes-telemetry', 'backbone',
  'agents', 'security', 'troubleshooting', 'glossary', 'faq',
] as const;

export function isHelpAudience(value: unknown): value is HelpAudience {
  return value === 'beginner' || value === 'advanced' || value === 'both';
}

export function audienceLabel(audience: HelpAudience): string {
  if (audience === 'advanced') return 'Advanced';
  if (audience === 'beginner') return 'Beginner';
  return 'Everyone';
}

export function readingTimeLabel(minutes: number): string {
  const value = Number.isFinite(minutes) && minutes > 0 ? Math.max(1, Math.round(minutes)) : 1;
  return `${value} min read`;
}

function toStringArray(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string' && item.length > 0) : [];
}

export function normalizeHelpTopicMeta(raw: unknown): HelpTopicMeta | null {
  if (!raw || typeof raw !== 'object') return null;
  const record = raw as Record<string, unknown>;
  const slug = typeof record.slug === 'string' ? record.slug.trim() : '';
  if (!slug) return null;
  const title = typeof record.title === 'string' && record.title.trim() ? record.title.trim() : slug.replace(/-/g, ' ');
  return {
    slug,
    title,
    summary: typeof record.summary === 'string' ? record.summary.trim() : '',
    audience: isHelpAudience(record.audience) ? record.audience : 'both',
    minutes: typeof record.minutes === 'number' && record.minutes > 0 ? record.minutes : 1,
    related: toStringArray(record.related),
  };
}

export function normalizeHelpIndex(raw: unknown): HelpIndex {
  const record = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  const rawTopics = Array.isArray(record.topics) ? record.topics : [];
  const seen = new Set<string>();
  const topics: HelpTopicMeta[] = [];
  for (const item of rawTopics) {
    const meta = normalizeHelpTopicMeta(item);
    if (meta && !seen.has(meta.slug)) {
      seen.add(meta.slug);
      topics.push(meta);
    }
  }
  return {
    version: typeof record.version === 'number' ? record.version : 1,
    topics,
  };
}

function estimateMinutes(markdown: string): number {
  const words = markdown.trim().split(/\s+/).filter(Boolean).length;
  return Math.max(1, Math.round(words / 220));
}

export function normalizeHelpTopic(raw: unknown, meta?: HelpTopicMeta | null): HelpTopic | null {
  let markdown = '';
  let record: Record<string, unknown> = {};
  if (typeof raw === 'string') {
    markdown = raw;
  } else if (raw && typeof raw === 'object') {
    record = raw as Record<string, unknown>;
    markdown = typeof record.markdown === 'string' ? record.markdown : '';
  }
  if (!markdown.trim()) return null;
  const derived = normalizeHelpTopicMeta({ ...record, ...(meta ?? {}) });
  if (!derived) return null;
  const titleFromBody = markdown.match(/^#\s+(.+)$/m)?.[1]?.trim();
  const summaryFromBody = markdown
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find((line) => line.length > 0 && !line.startsWith('#') && !line.startsWith('<!--'));
  return {
    ...derived,
    title: derived.title || titleFromBody || derived.slug,
    summary: derived.summary || summaryFromBody || '',
    minutes: typeof record.minutes === 'number' && record.minutes > 0 ? record.minutes : derived.minutes || estimateMinutes(markdown),
    markdown,
  };
}

export function audienceMatches(topicAudience: HelpAudience, filter: 'beginner' | 'advanced'): boolean {
  return topicAudience === 'both' || topicAudience === filter;
}

export interface HelpSearchOptions {
  query?: string;
  audience?: 'beginner' | 'advanced' | null;
  bodies?: Record<string, string>;
  limit?: number;
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

/** Fuzzy-ish search over title, summary and (when available) topic body. */
export function searchHelpTopics(topics: HelpTopicMeta[], options: HelpSearchOptions = {}): HelpTopicMeta[] {
  const query = (options.query ?? '').trim().toLowerCase();
  const audience = options.audience ?? null;
  const terms = query.split(/\s+/).filter(Boolean);
  const filtered = topics.filter((topic) => !audience || audienceMatches(topic.audience, audience));
  if (!terms.length) {
    const limit = options.limit ?? filtered.length;
    return filtered.slice(0, limit);
  }
  const scored = filtered
    .map((topic) => {
      const body = (options.bodies?.[topic.slug] ?? '').toLowerCase();
      const title = topic.title.toLowerCase();
      const summary = topic.summary.toLowerCase();
      let score = 0;
      for (const term of terms) {
        if (title.includes(term)) score += 6;
        if (summary.includes(term)) score += 3;
        if (body.includes(term)) score += 1;
        if (new RegExp(`(^|[^a-z0-9])${escapeRegExp(term)}`, 'i').test(title)) score += 2;
      }
      return { topic, score };
    })
    .filter((entry) => entry.score > 0)
    .sort((left, right) => right.score - left.score || left.topic.minutes - right.topic.minutes || left.topic.title.localeCompare(right.topic.title));
  const limit = options.limit ?? scored.length;
  return scored.slice(0, limit).map((entry) => entry.topic);
}

export function relatedTopics(index: HelpIndex, topic: HelpTopicMeta, limit = 3): HelpTopicMeta[] {
  const bySlug = new Map(index.topics.map((item) => [item.slug, item]));
  const related = topic.related.map((slug) => bySlug.get(slug)).filter((item): item is HelpTopicMeta => Boolean(item));
  if (related.length >= limit) return related.slice(0, limit);
  const seen = new Set([topic.slug, ...related.map((item) => item.slug)]);
  for (const candidate of index.topics) {
    if (related.length >= limit) break;
    if (!seen.has(candidate.slug) && audienceMatches(topic.audience, candidate.audience === 'advanced' ? 'advanced' : 'beginner')) {
      related.push(candidate);
    }
  }
  return related.slice(0, limit);
}

export function slugifyHeading(text: string): string {
  return text
    .toLowerCase()
    .replace(/<[^>]+>/g, '')
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .trim()
    .replace(/\s+/g, '-')
    .replace(/-+/g, '-')
    .slice(0, 80) || 'section';
}

export function extractToc(markdown: string): HelpTocEntry[] {
  const entries: HelpTocEntry[] = [];
  const used = new Map<string, number>();
  for (const line of markdown.split(/\r?\n/)) {
    const match = line.match(/^(#{2,3})\s+(.+?)\s*#*\s*$/);
    if (!match) continue;
    const level = match[1]!.length;
    const text = match[2]!.trim();
    const base = slugifyHeading(text);
    const count = used.get(base) ?? 0;
    used.set(base, count + 1);
    entries.push({ id: count === 0 ? base : `${base}-${count + 1}`, text, level });
  }
  return entries;
}

/** Renders a safe HTML subset (all raw HTML is escaped before formatting). */
export function renderMarkdown(markdown: string): string {
  const source = markdown.replace(/\r\n/g, '\n');
  const lines = source.split('\n');
  const html: string[] = [];
  const used = new Map<string, number>();
  let listType: 'ul' | 'ol' | null = null;
  let inCode = false;
  let codeLines: string[] = [];
  let paragraph: string[] = [];
  let tableRows: string[][] = [];

  const inline = (text: string): string => {
    let safe = text
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
    safe = safe.replace(/`([^`]+)`/g, '<code>$1</code>');
    safe = safe.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    safe = safe.replace(/(^|[^*])\*([^*]+)\*(?!\*)/g, '$1<em>$2</em>');
    safe = safe.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_match: string, label: string, href: string) => {
      const sibling = href.match(/^(?:\.\/)?([a-z0-9-]+)\.md(#[A-Za-z0-9-_]+)?$/);
      if (sibling) {
        const slug = sibling[1]!;
        const anchor = sibling[2] ?? '';
        return `<a href="/help/${slug}${anchor}" data-help-slug="${slug}" data-help-anchor="${anchor.replace('#', '')}">${label}</a>`;
      }
      const url = href.startsWith('#') || href.startsWith('/') || /^https?:\/\//.test(href) ? href : '#';
      const external = /^https?:\/\//.test(url) ? ' target="_blank" rel="noreferrer"' : '';
      return `<a href="${url.replace(/"/g, '%22')}"${external}>${label}</a>`;
    });
    return safe;
  };
  const closeList = () => {
    if (listType) {
      html.push(`</${listType}>`);
      listType = null;
    }
  };
  const closeParagraph = () => {
    if (paragraph.length) {
      html.push(`<p>${paragraph.map(inline).join(' ')}</p>`);
      paragraph = [];
    }
  };
  const flushTable = () => {
    if (!tableRows.length) return;
    const [head, ...body] = tableRows;
    html.push('<div class="help-table"><table><thead><tr>' + head.map((cell) => `<th>${inline(cell)}</th>`).join('') + '</tr></thead><tbody>');
    for (const row of body) {
      html.push('<tr>' + row.map((cell) => `<td>${inline(cell)}</td>`).join('') + '</tr>');
    }
    html.push('</tbody></table></div>');
    tableRows = [];
  };

  for (const rawLine of lines) {
    const line = rawLine.trimEnd();
    if (inCode) {
      if (line.trim().startsWith('```')) {
        html.push(`<pre><code>${codeLines.join('\n').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>`);
        inCode = false;
        codeLines = [];
      } else {
        codeLines.push(rawLine);
      }
      continue;
    }
    if (line.trim().startsWith('```')) {
      closeParagraph();
      closeList();
      flushTable();
      inCode = true;
      continue;
    }
    if (line.trim() === '') {
      closeParagraph();
      closeList();
      flushTable();
      continue;
    }
    const heading = line.match(/^(#{1,4})\s+(.+?)\s*#*\s*$/);
    if (heading) {
      closeParagraph();
      closeList();
      flushTable();
      const level = Math.min(4, heading[1]!.length);
      const text = heading[2]!.trim();
      if (level >= 2) {
        const base = slugifyHeading(text);
        const count = used.get(base) ?? 0;
        used.set(base, count + 1);
        const id = count === 0 ? base : `${base}-${count + 1}`;
        html.push(`<h${level} id="${id}">${inline(text)}</h${level}>`);
      } else {
        html.push(`<h1>${inline(text)}</h1>`);
      }
      continue;
    }
    if (/^\s*(---|\*\*\*)\s*$/.test(line)) {
      closeParagraph();
      closeList();
      flushTable();
      html.push('<hr />');
      continue;
    }
    const quote = line.match(/^>\s?(.*)$/);
    if (quote) {
      closeParagraph();
      closeList();
      flushTable();
      html.push(`<blockquote>${inline(quote[1]!)}</blockquote>`);
      continue;
    }
    const unordered = line.match(/^\s*[-*]\s+(.*)$/);
    const ordered = line.match(/^\s*\d+[.)]\s+(.*)$/);
    if (unordered || ordered) {
      closeParagraph();
      flushTable();
      const type: 'ul' | 'ol' = unordered ? 'ul' : 'ol';
      if (listType && listType !== type) closeList();
      if (!listType) {
        listType = type;
        html.push(`<${type}>`);
      }
      html.push(`<li>${inline((unordered ?? ordered)![1]!)}</li>`);
      continue;
    }
    const table = line.match(/^\|(.+)\|$/);
    if (table) {
      const cells = table[1]!.split('|').map((cell) => cell.trim());
      if (cells.every((cell) => /^:?-{2,}:?$/.test(cell) || cell === '')) continue;
      closeParagraph();
      closeList();
      tableRows.push(cells);
      continue;
    }
    if (tableRows.length) flushTable();
    paragraph.push(line.trim());
  }
  if (inCode) {
    html.push(`<pre><code>${codeLines.join('\n').replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')}</code></pre>`);
  }
  closeParagraph();
  closeList();
  flushTable();
  return html.join('\n');
}

export function parseHelpRoute(pathname: string, search: string, hash = ''): HelpRoute | null {
  const pathMatch = pathname.match(/^\/help(?:\/([A-Za-z0-9-]+))?\/?$/);
  const querySlug = new URLSearchParams(search).get('help');
  if (!pathMatch && !querySlug) return null;
  const slug = decodeURIComponent(pathMatch?.[1] ?? querySlug ?? '').trim() || null;
  const anchor = hash.replace(/^#/, '').trim() || null;
  return { slug, anchor };
}

export function helpRoutePath(slug?: string | null, anchor?: string | null): string {
  const base = slug ? `/help/${encodeURIComponent(slug)}` : '/help';
  return anchor ? `${base}#${anchor}` : base;
}

export function findHelpTopic(topics: HelpTopicMeta[], slug: string | null | undefined): HelpTopicMeta | null {
  if (!slug) return null;
  return topics.find((topic) => topic.slug === slug) ?? null;
}

/** Index used when /api/help or the bundled index is unreachable. */
export function fallbackHelpIndex(): HelpIndex {
  return {
    version: 1,
    topics: [
      { slug: 'getting-started', title: 'Your first mesh in 10 minutes', summary: 'A guided first run: install, name, region, channel and one message.', audience: 'beginner', minutes: 8, related: ['install', 'first-mesh', 'security'] },
      { slug: 'agents', title: 'Use your AI agent', summary: 'Connect an MCP client, choose a prompt tier and review everything it changes.', audience: 'advanced', minutes: 10, related: ['security', 'getting-started'] },
      { slug: 'troubleshooting', title: 'When something is broken', summary: 'A calm checklist for radios, tokens, backbones and deliveries.', audience: 'both', minutes: 12, related: ['faq', 'nodes-telemetry'] },
    ],
  };
}

export interface HelpFetchOptions {
  mode?: 'demo' | 'live';
  apiBaseUrl?: string;
  token?: string | null;
  fetchImpl?: typeof fetch;
  /** Base URL where the bundled help copy is served (``/help/`` in the app). */
  bundleBaseUrl?: string;
  timeoutMs?: number;
}

async function requestJsonOrText(url: string, options: HelpFetchOptions): Promise<{ kind: 'json' | 'text'; body: unknown }> {
  const fetchImpl = options.fetchImpl ?? fetch;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), options.timeoutMs ?? 8000);
  try {
    const response = await fetchImpl(url, {
      signal: controller.signal,
      headers: options.token ? { Authorization: `Bearer ${options.token}`, Accept: 'application/json, text/markdown, text/plain' } : { Accept: 'application/json, text/markdown, text/plain' },
    });
    if (!response.ok) throw new Error(`Help request failed (${response.status})`);
    const contentType = response.headers.get('content-type') ?? '';
    if (contentType.includes('json')) return { kind: 'json', body: (await response.json()) as unknown };
    const text = await response.text();
    try {
      return { kind: 'json', body: JSON.parse(text) as unknown };
    } catch {
      return { kind: 'text', body: text };
    }
  } finally {
    clearTimeout(timer);
  }
}

export function bundledHelpBaseUrl(base?: string): string {
  const root = base ?? '/';
  return `${root.replace(/\/?$/, '/')}help/`;
}

export async function loadHelpIndex(options: HelpFetchOptions = {}): Promise<HelpIndex> {
  const apiBase = options.apiBaseUrl?.replace(/\/$/, '');
  if (options.mode === 'live' && apiBase) {
    try {
      const result = await requestJsonOrText(`${apiBase}/api/help`, options);
      return normalizeHelpIndex(result.body);
    } catch {
      // Offline help is a feature: fall through to the bundled corpus.
    }
  }
  try {
    const result = await requestJsonOrText(`${bundledHelpBaseUrl(options.bundleBaseUrl)}index.json`, options);
    return normalizeHelpIndex(result.body);
  } catch {
    return fallbackHelpIndex();
  }
}

export async function loadHelpTopic(
  meta: HelpTopicMeta,
  options: HelpFetchOptions = {},
): Promise<HelpTopic | null> {
  const apiBase = options.apiBaseUrl?.replace(/\/$/, '');
  if (options.mode === 'live' && apiBase) {
    try {
      const result = await requestJsonOrText(`${apiBase}/api/help/${encodeURIComponent(meta.slug)}`, options);
      const topic = normalizeHelpTopic(result.body, meta);
      if (topic) return topic;
    } catch {
      // fall through
    }
  }
  try {
    const result = await requestJsonOrText(`${bundledHelpBaseUrl(options.bundleBaseUrl)}${encodeURIComponent(meta.slug)}.md`, options);
    const body = result.kind === 'json' ? result.body : result.body;
    return normalizeHelpTopic(body, meta);
  } catch {
    return null;
  }
}

/** Loads every topic body for client-side search (bounded concurrency). */
export async function loadHelpCorpus(
  index: HelpIndex,
  options: HelpFetchOptions = {},
): Promise<Record<string, string>> {
  const bodies: Record<string, string> = {};
  const queue = [...index.topics];
  const workers = Array.from({ length: Math.min(4, queue.length) }, async () => {
    for (;;) {
      const meta = queue.shift();
      if (!meta) return;
      const topic = await loadHelpTopic(meta, options);
      if (topic) bodies[meta.slug] = topic.markdown;
    }
  });
  await Promise.all(workers);
  return bodies;
}
