import { act, renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ApiError } from '../../../api/client';
import type { Rule } from '../../../api/rulesTypes';
import type { FieldError } from '../../../api/types';
import { RULE_CATALOGUE } from './catalogue';
import { useRuleDraft, type RuleDraftOptions } from './useRuleDraft';

const STAMP = '2026-11-21T10:00:00.000Z';

function ruleOf(overrides: Partial<Rule> = {}): Rule {
  return {
    id: '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a51',
    name: 'Mark paid',
    enabled: true,
    condition: { all: [{ value: 'request.header.x-event', op: 'eq', arg: 'paid' }] },
    steps: [{ kind: 'http', method: 'POST', url: 'https://crm.test/hook', headers: [], body: null }],
    position: 1,
    created_at: STAMP,
    updated_at: STAMP,
    ...overrides,
  };
}

function failure(status: number, details: FieldError[] = []): ApiError {
  return new ApiError({ status, code: null, message: 'failed', details, retryAfter: null });
}

function setup(overrides: Partial<RuleDraftOptions> = {}) {
  const rule = ruleOf();
  const refresh = vi.fn(() => Promise.resolve<Rule | null>(rule));
  const write = vi.fn((input: { name?: string }) => Promise.resolve(ruleOf({ name: input.name ?? '' })));
  const options: Omit<RuleDraftOptions, 'source'> = {
    initial: {},
    creating: false,
    startDirty: false,
    catalogue: RULE_CATALOGUE,
    readOnly: false,
    refresh,
    write,
    ...overrides,
  };
  const view = renderHook(({ source }) => useRuleDraft({ ...options, source }), {
    initialProps: { source: rule as Rule | null },
  });
  return { ...view, refresh, write, rule };
}

describe('useRuleDraft', () => {
  it('is dirty only while the draft differs from the saved rule and exposes the API form', () => {
    const { result } = setup();
    expect(result.current.dirty).toBe(false);

    act(() => {
      result.current.setName('Mark paid orders');
    });
    expect(result.current.dirty).toBe(true);
    expect(result.current.input.name).toBe('Mark paid orders');
    expect(result.current.input.steps).toEqual([
      {
        kind: 'http',
        method: 'POST',
        url: 'https://crm.test/hook',
        headers: [],
        follow_redirects: true,
        timeout_ms: 10000,
      },
    ]);

    act(() => {
      result.current.setName('Mark paid');
    });
    expect(result.current.dirty).toBe(false);
  });

  it('blocks the save on a client error and asks for focus on the first one', async () => {
    const { result, write } = setup();
    act(() => {
      result.current.setName('   ');
    });
    await act(async () => {
      await result.current.save();
    });

    expect(write).not.toHaveBeenCalled();
    expect(result.current.attempted).toBe(true);
    expect(result.current.validation.nameMissing).toBe(true);
    expect(result.current.focusRequest?.target).toEqual({ kind: 'name' });
  });

  it('saves after checking the stored revision and adopts the answer as the new baseline', async () => {
    const { result, refresh, write } = setup();
    act(() => {
      result.current.setName('Mark paid orders');
    });
    await act(async () => {
      await result.current.save();
    });

    expect(refresh).toHaveBeenCalledTimes(1);
    expect(write).toHaveBeenCalledTimes(1);
    expect(write.mock.calls[0]?.[0].name).toBe('Mark paid orders');
    expect(result.current.dirty).toBe(false);
    expect(result.current.status?.kind).toBe('saved');
  });

  it('reports a conflict without writing when the stored rule changed, and reload takes theirs', async () => {
    const theirs = ruleOf({ name: 'Renamed elsewhere', updated_at: '2026-11-21T11:00:00.000Z' });
    const { result, write, rerender } = setup({ refresh: vi.fn(() => Promise.resolve<Rule | null>(theirs)) });
    act(() => {
      result.current.setName('Mine');
    });
    await act(async () => {
      await result.current.save();
    });

    expect(write).not.toHaveBeenCalled();
    expect(result.current.conflict).toBe(true);

    rerender({ source: theirs });
    act(() => {
      result.current.reload();
    });
    expect(result.current.conflict).toBe(false);
    expect(result.current.draft.name).toBe('Renamed elsewhere');
    expect(result.current.dirty).toBe(false);
  });

  it('treats a 409 from the server as a conflict and keep mine lets the next save through', async () => {
    const write = vi
      .fn<(input: { name?: string }) => Promise<Rule>>()
      .mockRejectedValueOnce(failure(409))
      .mockImplementation((input) => Promise.resolve(ruleOf({ name: input.name ?? '' })));
    const { result } = setup({ write });
    act(() => {
      result.current.setName('Mine');
    });
    await act(async () => {
      await result.current.save();
    });
    expect(result.current.conflict).toBe(true);

    act(() => {
      result.current.overwrite();
    });
    expect(result.current.conflict).toBe(false);
    await act(async () => {
      await result.current.save();
    });
    expect(write).toHaveBeenCalledTimes(2);
    expect(result.current.dirty).toBe(false);
  });

  it('maps 422 field errors onto the name, the condition row and the step field', async () => {
    const details: FieldError[] = [
      { field: 'name', code: 'duplicate', message: 'is already used' },
      { field: 'condition.all.0.value', code: 'invalid_format', message: 'is not a variable path' },
      { field: 'steps.0.url', code: 'invalid_format', message: 'host is not allowed' },
      { field: 'steps', code: 'too_many', message: 'has too many steps' },
    ];
    const { result } = setup({ write: vi.fn(() => Promise.reject(failure(422, details))) });
    act(() => {
      result.current.setName('Mine');
    });
    await act(async () => {
      await result.current.save();
    });

    const { server, draft } = result.current;
    expect(server.name.map((error) => error.message)).toEqual(['is already used']);
    expect(server.steps).toEqual([{ stepId: draft.steps[0]?.id, field: 'url', message: 'host is not allowed' }]);
    const [rowId] = [...server.rows.keys()];
    expect(server.rows.get(rowId ?? '')?.[0]?.message).toBe('is not a variable path');
    expect(server.general.map((error) => error.message)).toEqual(['has too many steps']);
    expect(result.current.focusRequest?.target).toEqual({ kind: 'name' });
    expect(result.current.saveFailed).toBe(false);
  });
});
