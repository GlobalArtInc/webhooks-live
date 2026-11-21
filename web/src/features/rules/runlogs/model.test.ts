import { describe, expect, it } from 'vitest';
import { jsonCaptureFixture } from '../../../test/fixtures/captures';
import { captureRunsFixture, rerunFailedFixture, ruleRunFixture, RUN_FIXTURE_ID } from './fixtures';
import {
  CAPTURE_RUN_LIMIT,
  groupEntries,
  MAX_OUTPUT_CHARS,
  outputText,
  responseFacts,
  upsertRunInPages,
  withRuleRun,
  type RuleRunPages,
} from './model';

const PAGES: RuleRunPages = {
  pages: [{ items: captureRunsFixture, next_cursor: null }],
  pageParams: [null],
};

describe('groupEntries', () => {
  it('groups steps under their rule and keeps the rule order of the run', () => {
    const groups = groupEntries(ruleRunFixture().entries);

    expect(groups.map((group) => [group.ruleName, group.matched, group.steps.length])).toEqual([
      ['Mark paid orders', true, 3],
      ['Reject bad signature', false, 0],
    ]);
  });
});

describe('outputText', () => {
  it('bounds long output and marks it as truncated', () => {
    const result = outputText({ body: 'x'.repeat(MAX_OUTPUT_CHARS * 2) });

    expect(result?.truncated).toBe(true);
    expect(result?.text.length).toBe(MAX_OUTPUT_CHARS);
  });
});

describe('responseFacts', () => {
  it('names the rule that answered and reports the error of the capture run', () => {
    const facts = responseFacts(200, [rerunFailedFixture, ruleRunFixture({ error: 'http: timeout after 5 s' })]);

    expect(facts).toEqual({ status: 200, ruleError: 'http: timeout after 5 s', changedBy: 'Mark paid orders' });
  });
});

describe('upsertRunInPages', () => {
  it('drops a run that no longer matches the state filter', () => {
    const next = upsertRunInPages(PAGES, ruleRunFixture({ id: RUN_FIXTURE_ID, state: 'succeeded' }), {
      state: 'failed',
    });

    expect(next.pages[0]?.items.map((run) => run.id)).toEqual([rerunFailedFixture.id]);
  });

  it('prepends a new run to the first page', () => {
    const fresh = ruleRunFixture({ id: '0193a1c2-9d10-7a3e-8b44-1c2d3e4f6a09' });
    const next = upsertRunInPages(PAGES, fresh, {});

    expect(next.pages[0]?.items.map((run) => run.id)).toEqual([fresh.id, rerunFailedFixture.id, RUN_FIXTURE_ID]);
  });
});

describe('withRuleRun', () => {
  it('keeps the capture detail at the server limit of runs', () => {
    const runs = Array.from({ length: CAPTURE_RUN_LIMIT }, (_value, index) =>
      ruleRunFixture({ id: `0193a1c2-9d10-7a3e-8b44-1c2d3e4f6${String(index).padStart(3, '0')}` }),
    );
    const capture = withRuleRun({ ...jsonCaptureFixture, rule_runs: runs }, rerunFailedFixture);

    expect(capture.rule_runs?.length).toBe(CAPTURE_RUN_LIMIT);
    expect(capture.rule_runs?.[0]?.id).toBe(rerunFailedFixture.id);
  });
});
