import { describe, expect, it } from 'vitest';
import { RULE_IDS, rulesFixture } from '../test/fixtures/rules';
import type { RuleList } from './rulesTypes';
import { withEnabled, withOrder, withoutRule, withRule } from './rules';

function fixtureList(): RuleList {
  return { items: rulesFixture(), version: 4 };
}

describe('rule list cache helpers', () => {
  it('reorders by id and keeps the version', () => {
    const next = withOrder(fixtureList(), [RULE_IDS.mail, RULE_IDS.markPaid, RULE_IDS.reject]);

    expect(next.items.map((rule) => rule.id)).toEqual([RULE_IDS.mail, RULE_IDS.markPaid, RULE_IDS.reject]);
    expect(next.version).toBe(4);
  });

  it('switches one rule and leaves the list it came from unchanged', () => {
    const list = fixtureList();
    const next = withEnabled(list, RULE_IDS.mail, true);

    expect(next.items.map((rule) => rule.enabled)).toEqual([true, true, true]);
    expect(list.items[2]?.enabled).toBe(false);
  });

  it('replaces a rule with the server copy and removes a deleted one', () => {
    const [first] = fixtureList().items;
    if (first === undefined) {
      throw new Error('fixture has no rules');
    }
    const renamed = withRule(fixtureList(), { ...first, name: 'Renamed' });

    expect(renamed.items[0]?.name).toBe('Renamed');
    expect(withoutRule(fixtureList(), RULE_IDS.reject).items.map((rule) => rule.id)).toEqual([
      RULE_IDS.markPaid,
      RULE_IDS.mail,
    ]);
  });
});
