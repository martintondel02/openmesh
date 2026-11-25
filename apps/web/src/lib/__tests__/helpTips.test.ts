import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { CONTROL_REGISTRY, controlEntriesFor, unusedHelpTips, validateControlRegistry } from '../control-registry';
import { HELP_TIPS, helpTipContent, missingHelpKeys } from '../help-tips';

const index = JSON.parse(readFileSync(resolve(process.cwd(), '../../content/help/index.json'), 'utf8')) as {
  topics: { slug: string }[];
};
const slugs = new Set(index.topics.map((topic) => topic.slug));

describe('help tips + control registry', () => {
  it('registers every settings/backbone/agents control with a help key', () => {
    expect(controlEntriesFor('settings').length).toBeGreaterThanOrEqual(18);
    expect(controlEntriesFor('backbone').length).toBeGreaterThanOrEqual(16);
    expect(controlEntriesFor('agents').length).toBeGreaterThanOrEqual(8);
    expect(controlEntriesFor('onboarding').length).toBeGreaterThanOrEqual(5);
    const ids = CONTROL_REGISTRY.map((entry) => entry.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('has no missing or broken helpKey/topic link', () => {
    expect(validateControlRegistry(slugs)).toEqual([]);
    expect(missingHelpKeys(CONTROL_REGISTRY.map((entry) => entry.helpKey))).toEqual([]);
  });

  it('writes every tip with a what, a why and a real help topic', () => {
    for (const entry of CONTROL_REGISTRY) {
      const tip = helpTipContent(entry.helpKey);
      expect(tip, entry.helpKey).not.toBeNull();
      expect(tip!.what.length, entry.helpKey).toBeGreaterThan(10);
      expect(tip!.why.length, entry.helpKey).toBeGreaterThan(10);
      expect(slugs.has(tip!.topic), `${entry.helpKey} → ${tip!.topic}`).toBe(true);
    }
  });

  it('leaves only generic/global tips unreferenced', () => {
    expect(unusedHelpTips()).toEqual([]);
    expect(Object.keys(HELP_TIPS).length).toBeGreaterThanOrEqual(CONTROL_REGISTRY.length);
  });
});
