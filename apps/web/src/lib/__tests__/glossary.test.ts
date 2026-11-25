import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { GLOSSARY_TERMS, glossaryDefinitions, glossaryEntry, normalizeTermKey } from '../glossary';

const index = JSON.parse(readFileSync(resolve(process.cwd(), '../../content/help/index.json'), 'utf8')) as {
  topics: { slug: string }[];
};
const slugs = new Set(index.topics.map((topic) => topic.slug));

describe('glossary', () => {
  it('has at least 30 unique terms', () => {
    expect(GLOSSARY_TERMS.length).toBeGreaterThanOrEqual(30);
    const keys = GLOSSARY_TERMS.map((entry) => normalizeTermKey(entry.term));
    expect(new Set(keys).size).toBe(keys.length);
  });

  it('resolves terms case-insensitively and by short label', () => {
    expect(glossaryEntry('lora')?.term).toBe('LoRa');
    expect(glossaryEntry('Node ID')?.term).toBe('Node ID');
    expect(glossaryEntry('  store-and-forward  ')?.term).toBe('Store-and-forward');
    expect(glossaryEntry('Long-range radio')?.term).toBe('LoRa');
    expect(glossaryEntry('not-a-real-term')).toBeNull();
  });

  it('has a definition and a valid help topic for every entry', () => {
    for (const entry of GLOSSARY_TERMS) {
      expect(entry.definition.length, entry.term).toBeGreaterThan(10);
      expect(entry.short.length, entry.term).toBeGreaterThan(2);
      expect(slugs.has(entry.helpTopic), `${entry.term} → ${entry.helpTopic}`).toBe(true);
    }
  });

  it('filters unknown terms while keeping order', () => {
    expect(glossaryDefinitions(['LoRa', 'nope', 'Airtime']).map((entry) => entry.term)).toEqual(['LoRa', 'Airtime']);
  });
});
