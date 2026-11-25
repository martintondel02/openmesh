import { readFileSync, readdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  audienceLabel,
  audienceMatches,
  extractToc,
  fallbackHelpIndex,
  findHelpTopic,
  helpRoutePath,
  isHelpAudience,
  loadHelpIndex,
  loadHelpTopic,
  normalizeHelpIndex,
  normalizeHelpTopic,
  parseHelpRoute,
  readingTimeLabel,
  relatedTopics,
  renderMarkdown,
  searchHelpTopics,
  slugifyHeading,
} from '../help';

const helpDir = resolve(process.cwd(), '../../content/help');
const rawIndex = JSON.parse(readFileSync(resolve(helpDir, 'index.json'), 'utf8')) as unknown;
const index = normalizeHelpIndex(rawIndex);
const meta = index.topics;
const files = readdirSync(helpDir).filter((file) => file.endsWith('.md'));

describe('help index', () => {
  it('loads all 16 topics from the normative corpus', () => {
    expect(index.version).toBe(1);
    expect(meta).toHaveLength(16);
    expect(files).toHaveLength(16);
    expect(meta.map((topic) => topic.slug).sort()).toEqual(files.map((file) => file.replace('.md', '')).sort());
  });

  it('keeps metadata identical to index.json', () => {
    for (const topic of meta) {
      expect(isHelpAudience(topic.audience)).toBe(true);
      expect(topic.minutes).toBeGreaterThan(0);
      for (const related of topic.related) {
        expect(meta.some((candidate) => candidate.slug === related), `${topic.slug} → ${related}`).toBe(true);
      }
    }
  });

  it('normalises malformed payloads safely', () => {
    const normalized = normalizeHelpIndex({ topics: [{ slug: 'x' }, null, { title: 'no slug' }, { slug: 'x' }] });
    expect(normalized.topics).toHaveLength(1);
    expect(normalized.topics[0]).toMatchObject({ slug: 'x', audience: 'both' });
    expect(normalizeHelpIndex(null).topics).toEqual([]);
    expect(fallbackHelpIndex().topics.length).toBeGreaterThan(0);
  });
});

describe('help corpus rendering', () => {
  it('renders 100 % of content/help topics as non-empty markdown', () => {
    for (const topic of meta) {
      const markdown = readFileSync(resolve(helpDir, `${topic.slug}.md`), 'utf8');
      const normalized = normalizeHelpTopic({ ...topic, markdown }, topic);
      expect(normalized, topic.slug).not.toBeNull();
      expect(normalized!.title, topic.slug).toBe(topic.title);
      const html = renderMarkdown(markdown);
      expect(html.length, topic.slug).toBeGreaterThan(200);
      expect(html, topic.slug).toContain('<h1>');
      expect(html, topic.slug).not.toContain('<script');
    }
  });

  it('turns sibling .md links into in-app help links', () => {
    const markdown = 'See [channels](channels.md) and [security notes](security.md#tokens).';
    const html = renderMarkdown(markdown);
    expect(html).toContain('href="/help/channels"');
    expect(html).toContain('data-help-slug="channels"');
    expect(html).toContain('href="/help/security#tokens"');
    expect(html).toContain('data-help-anchor="tokens"');
  });

  it('renders headings with stable ids, lists, code, tables and inline formatting', () => {
    expect(slugifyHeading('What is a Node ID?')).toBe('what-is-a-node-id');
    const html = renderMarkdown(
      ['## Why it matters', '', 'A **bold** and *calm* paragraph with `code`.', '', '- one', '- two', '', '```', 'a < b', '```', '', '| A | B |', '| --- | --- |', '| 1 | 2 |'].join('\n'),
    );
    expect(html).toContain('<h2 id="why-it-matters">Why it matters</h2>');
    expect(html).toContain('<strong>bold</strong>');
    expect(html).toContain('<em>calm</em>');
    expect(html).toContain('<code>code</code>');
    expect(html).toContain('<ul>');
    expect(html).toContain('<pre><code>a &lt; b</code></pre>');
    expect(html).toContain('<table>');
    expect(html).toContain('<th>A</th>');
  });

  it('escapes raw HTML and unsafe link protocols', () => {
    const html = renderMarkdown('<script>alert(1)</script> [bad](javascript:alert(1))');
    expect(html).not.toContain('<script>');
    expect(html).toContain('&lt;script&gt;');
    expect(html).toContain('href="#"');
    expect(renderMarkdown('[ok](https://example.org)')).toContain('rel="noreferrer"');
  });

  it('extracts a table of contents with deduplicated anchors', () => {
    const toc = extractToc('# Title\n\n## Same\n\n### Same\n\n## Another');
    expect(toc).toEqual([
      { id: 'same', text: 'Same', level: 2 },
      { id: 'same-2', text: 'Same', level: 3 },
      { id: 'another', text: 'Another', level: 2 },
    ]);
  });
});

describe('help search and deep links', () => {
  it('filters by audience, counting "both" for everyone', () => {
    expect(audienceMatches('both', 'beginner')).toBe(true);
    expect(audienceMatches('both', 'advanced')).toBe(true);
    expect(audienceMatches('advanced', 'beginner')).toBe(false);
    expect(audienceLabel('both')).toBe('Everyone');
    expect(readingTimeLabel(8)).toBe('8 min read');
    expect(readingTimeLabel(0)).toBe('1 min read');
  });

  it('searches title, summary and body and respects the audience filter', () => {
    const byTitle = searchHelpTopics(meta, { query: 'backbone' });
    expect(byTitle[0]?.slug).toBe('backbone');
    const advancedOnly = searchHelpTopics(meta, { query: 'agent', audience: 'advanced' });
    expect(advancedOnly.some((topic) => topic.slug === 'agents')).toBe(true);
    const beginnerOnly = searchHelpTopics(meta, { query: 'agent', audience: 'beginner' });
    expect(beginnerOnly.some((topic) => topic.slug === 'agents')).toBe(false);
    const body = { 'getting-started': 'install the radio then invite someone over the air' };
    expect(searchHelpTopics(meta, { query: 'radio invite', bodies: body }).length).toBeGreaterThan(0);
    expect(searchHelpTopics(meta, { query: 'zzzznotfound' })).toEqual([]);
    expect(searchHelpTopics(meta, { query: '' })).toHaveLength(meta.length);
  });

  it('resolves related topics and falls back within the audience', () => {
    const backbone = meta.find((topic) => topic.slug === 'backbone')!;
    const related = relatedTopics(index, backbone, 3);
    expect(related.length).toBe(3);
    expect(related.every((topic) => topic.slug !== 'backbone')).toBe(true);
    expect(findHelpTopic(meta, 'backbone')?.title).toBe('Run a backbone');
    expect(findHelpTopic(meta, 'missing')).toBeNull();
  });

  it('parses and builds help routes with anchors', () => {
    expect(parseHelpRoute('/help', '')).toEqual({ slug: null, anchor: null });
    expect(parseHelpRoute('/help/security', '', '#tokens')).toEqual({ slug: 'security', anchor: 'tokens' });
    expect(parseHelpRoute('/help/security/', '')).toEqual({ slug: 'security', anchor: null });
    expect(parseHelpRoute('/chats', '?help=agents')).toEqual({ slug: 'agents', anchor: null });
    expect(parseHelpRoute('/chats', '')).toBeNull();
    expect(helpRoutePath('security', 'tokens')).toBe('/help/security#tokens');
    expect(helpRoutePath()).toBe('/help');
  });
});

describe('help loaders', () => {
  const topic = meta.find((entry) => entry.slug === 'getting-started')!;
  const jsonResponse = (payload: unknown) =>
    new Response(JSON.stringify(payload), { status: 200, headers: { 'Content-Type': 'application/json' } });

  it('loads the bundled index and topic in demo mode', async () => {
    const calls: string[] = [];
    const fetchImpl = (async (input: RequestInfo | URL) => {
      const url = String(input);
      calls.push(url);
      if (url.endsWith('index.json')) return jsonResponse({ version: 1, topics: [topic] });
      return new Response(`# ${topic.title}\n\nBundled body`, { status: 200, headers: { 'Content-Type': 'text/markdown' } });
    }) as typeof fetch;
    const loadedIndex = await loadHelpIndex({ mode: 'demo', fetchImpl, bundleBaseUrl: '/' });
    expect(loadedIndex.topics[0]?.slug).toBe(topic.slug);
    const loadedTopic = await loadHelpTopic(topic, { mode: 'demo', fetchImpl, bundleBaseUrl: '/' });
    expect(loadedTopic?.markdown).toContain('Bundled body');
    expect(calls.some((url) => url.endsWith('/help/index.json'))).toBe(true);
  });

  it('uses the bridge API in live mode and falls back to bundled copy when it fails', async () => {
    const calls: string[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      calls.push(url);
      if (url.endsWith('/api/help')) {
        expect((init?.headers as Record<string, string>).Authorization).toBe('Bearer secret');
        return jsonResponse({ topics: [topic] });
      }
      if (url.endsWith(`/api/help/${topic.slug}`)) {
        return jsonResponse({ ...topic, markdown: '# API topic\n\nFrom the bridge' });
      }
      if (url.endsWith('index.json')) return jsonResponse({ topics: [topic] });
      return new Response('# fallback\n\nlocal', { status: 200 });
    }) as typeof fetch;
    const loaded = await loadHelpIndex({ mode: 'live', apiBaseUrl: 'http://127.0.0.1:8787', token: 'secret', fetchImpl, bundleBaseUrl: '/' });
    expect(loaded.topics[0]?.slug).toBe(topic.slug);
    const loadedTopic = await loadHelpTopic(topic, { mode: 'live', apiBaseUrl: 'http://127.0.0.1:8787', token: 'secret', fetchImpl, bundleBaseUrl: '/' });
    expect(loadedTopic?.markdown).toContain('From the bridge');

    const failing = (async () => new Response('nope', { status: 500 })) as typeof fetch;
    const fallback = await loadHelpIndex({ mode: 'live', apiBaseUrl: 'http://127.0.0.1:8787', fetchImpl: failing, bundleBaseUrl: '/' });
    expect(fallback.topics.length).toBeGreaterThan(0);
  });
});
