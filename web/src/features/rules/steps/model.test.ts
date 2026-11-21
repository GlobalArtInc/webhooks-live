import { describe, expect, it } from 'vitest';
import { STEP_CATALOGUE } from './fixtures';
import {
  changeGuardOperator,
  createStep,
  fieldIssue,
  formatDuration,
  guardArgumentFromText,
  guardArgumentToText,
  insertStep,
  isGuardComplete,
  moveStep,
  producedVariables,
  removeStep,
  renderSummary,
  validateStep,
} from './model';
import type { RuleStep, StepCatalogueEntry, StepFieldSchema } from './types';

function entryOf(kind: string): StepCatalogueEntry {
  const entry = STEP_CATALOGUE.find((candidate) => candidate.kind === kind);
  if (entry === undefined) {
    throw new Error(`fixture entry ${kind} is missing`);
  }
  return entry;
}

function fieldOf(kind: string, name: string): StepFieldSchema {
  const field = entryOf(kind).fields.find((candidate) => candidate.name === name);
  if (field === undefined) {
    throw new Error(`fixture field ${kind}.${name} is missing`);
  }
  return field;
}

function stepOf(kind: string, id: string): RuleStep {
  return createStep(entryOf(kind), id);
}

describe('step list edits', () => {
  const steps = [stepOf('delay', 'a'), stepOf('stop', 'b'), stepOf('delay', 'c')];

  it('inserts between two steps and clamps the index', () => {
    const added = stepOf('stop', 'x');
    expect(insertStep(steps, 1, added).map((step) => step.id)).toEqual(['a', 'x', 'b', 'c']);
    expect(insertStep(steps, 99, added).map((step) => step.id)).toEqual(['a', 'b', 'c', 'x']);
    expect(insertStep(steps, -4, added).map((step) => step.id)).toEqual(['x', 'a', 'b', 'c']);
  });

  it('moves a step and ignores moves past either end', () => {
    expect(moveStep(steps, 0, 1).map((step) => step.id)).toEqual(['b', 'a', 'c']);
    expect(moveStep(steps, 0, -1).map((step) => step.id)).toEqual(['a', 'b', 'c']);
    expect(moveStep(steps, 2, 1).map((step) => step.id)).toEqual(['a', 'b', 'c']);
  });

  it('removes a step by position', () => {
    expect(removeStep(steps, 1).map((step) => step.id)).toEqual(['a', 'c']);
  });
});

describe('createStep', () => {
  it('fills every schema field with its default', () => {
    const step = stepOf('http', 'h');
    expect(step.fields).toEqual({
      method: 'GET',
      url: '',
      headers: [],
      body: null,
      timeout_ms: 10000,
      follow_redirects: true,
    });
    expect(step.name).toBeNull();
    expect(step.guard).toBeNull();
    expect(step.continueOnError).toBe(false);
  });

  it('gives each new step its own id', () => {
    expect(createStep(entryOf('stop')).id).not.toBe(createStep(entryOf('stop')).id);
  });
});

describe('summary and produced variables', () => {
  it('fills the summary template and marks empty values', () => {
    const step = stepOf('extract', 'e');
    expect(renderSummary(entryOf('extract'), step)).toBe('from … into …');
    step.fields.from = 'request.body';
    step.fields.into = 'amount';
    expect(renderSummary(entryOf('extract'), step)).toBe('from request.body into amount');
    expect(renderSummary(entryOf('stop'), stepOf('stop', 's'))).toBeNull();
  });

  it('lists a variable only once every placeholder has a value', () => {
    const step = stepOf('extract', 'e');
    expect(producedVariables(entryOf('extract'), step)).toEqual([]);
    step.fields.into = 'amount';
    expect(producedVariables(entryOf('extract'), step)).toEqual(['var.amount']);
  });

  it('builds step result names from the step name', () => {
    const step = stepOf('http', 'h');
    expect(producedVariables(entryOf('http'), step)).toEqual([]);
    step.name = 'crm';
    expect(producedVariables(entryOf('http'), step)).toEqual(['step.crm.status', 'step.crm.body']);
  });
});

describe('guards', () => {
  it('converts the argument text by operator', () => {
    expect(guardArgumentFromText('gt', '12')).toBe(12);
    expect(guardArgumentFromText('gt', 'twelve')).toBe('twelve');
    expect(guardArgumentFromText('in', 'a, b,, c ')).toEqual(['a', 'b', 'c']);
    expect(guardArgumentFromText('exists', 'ignored')).toBeNull();
    expect(guardArgumentToText(['a', 'b'])).toBe('a, b');
    expect(guardArgumentToText(null)).toBe('');
  });

  it('keeps the text when the operator changes', () => {
    const next = changeGuardOperator({ value: 'var.x', op: 'eq', arg: '5' }, 'gt');
    expect(next).toEqual({ value: 'var.x', op: 'gt', arg: 5 });
    expect(changeGuardOperator(next, 'exists').arg).toBeNull();
  });

  it('is complete only with a subject and, when needed, an argument', () => {
    expect(isGuardComplete({ value: '', op: 'eq', arg: 'a' })).toBe(false);
    expect(isGuardComplete({ value: 'var.x', op: 'eq', arg: '' })).toBe(false);
    expect(isGuardComplete({ value: 'var.x', op: 'in', arg: [] })).toBe(false);
    expect(isGuardComplete({ value: 'var.x', op: 'exists', arg: null })).toBe(true);
    expect(isGuardComplete({ value: 'var.x', op: 'gt', arg: 0 })).toBe(true);
  });
});

describe('field shape checks', () => {
  it('accepts values on the limit and rejects the next one', () => {
    const delay = fieldOf('delay', 'ms');
    expect(fieldIssue(delay, 10000)).toBeNull();
    expect(fieldIssue(delay, 10001)).toEqual({ code: 'tooBig', limit: 10000 });
    expect(fieldIssue(delay, -1)).toEqual({ code: 'tooSmall', limit: 0 });
    expect(fieldIssue(delay, 1.5)).toEqual({ code: 'notInteger' });
    expect(fieldIssue(delay, null)).toBeNull();
  });

  it('checks header names, JSON paths and lengths', () => {
    expect(fieldIssue(fieldOf('set_header', 'name'), 'X-Order')).toBeNull();
    expect(fieldIssue(fieldOf('set_header', 'name'), 'X Order')).toEqual({ code: 'badHeaderName' });
    expect(fieldIssue(fieldOf('extract', 'json_path'), '$.a')).toBeNull();
    expect(fieldIssue(fieldOf('extract', 'json_path'), 'a.b')).toEqual({ code: 'badJsonPath' });
    expect(fieldIssue(fieldOf('extract', 'into'), 'x'.repeat(65))).toEqual({ code: 'tooLong', limit: 64 });
    expect(fieldIssue(fieldOf('script', 'source'), 'x'.repeat(16 * 1024 + 1))).toEqual({
      code: 'tooLarge',
      limit: 16 * 1024,
    });
  });

  it('validates the step name and its uniqueness', () => {
    const step = stepOf('stop', 's');
    step.name = 'Bad Name';
    expect(validateStep(entryOf('stop'), step, [])).toEqual([{ field: 'name', issue: { code: 'badName' } }]);
    step.name = 'crm';
    expect(validateStep(entryOf('stop'), step, ['crm'])).toEqual([{ field: 'name', issue: { code: 'duplicateName' } }]);
    expect(validateStep(entryOf('stop'), step, ['other'])).toEqual([]);
  });

  it('flags an enabled but empty guard', () => {
    const step = stepOf('stop', 's');
    step.guard = { value: '', op: 'eq', arg: '' };
    expect(validateStep(entryOf('stop'), step, [])).toEqual([{ field: 'guard', issue: { code: 'guardIncomplete' } }]);
  });
});

describe('formatDuration', () => {
  it('uses milliseconds below one second and seconds above', () => {
    expect(formatDuration(250, 'en')).toContain('250');
    expect(formatDuration(1500, 'en')).toContain('1.5');
  });
});
