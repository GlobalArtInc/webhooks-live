import { describe, expect, it } from 'vitest';
import type { ResponseSettingsDraft } from '../workspace/response/responseSettings';
import { parseCatalogue, parsePreview } from './api';
import { groupDiagnostics, problemItems, sortProblems } from './diagnostics';
import { languageOf } from './language';
import { DEFAULT_PANEL_PREFS, parsePanelPrefs } from './panelPrefs';
import { previewRequestOf } from './previewRequest';
import { validatePreviewSearch } from './previewSearch';
import type { PreviewDiagnostic } from './types';

function diagnostic(overrides: Partial<PreviewDiagnostic>): PreviewDiagnostic {
  return {
    field: 'body',
    code: 'template_invalid',
    severity: 'error',
    line: 1,
    column: 1,
    length: 1,
    message: 'Problem',
    ...overrides,
  };
}

function draftOf(overrides: Partial<ResponseSettingsDraft>): ResponseSettingsDraft {
  return {
    status: '200',
    contentType: ' application/json ',
    body: '{}',
    bodyEncoding: 'utf8',
    delay: '0',
    cors: false,
    overflow: 'refuse',
    templated: true,
    headers: [{ key: 1, name: ' x-order ', value: ' {{ request.path }} ' }],
    ...overrides,
  };
}

describe('diagnostics', () => {
  it('routes body, header value and other fields to their targets and ignores header names', () => {
    const items = problemItems([
      diagnostic({ field: 'response.body', line: 4, column: 19 }),
      diagnostic({ field: 'headers.1.value', severity: 'warning' }),
      diagnostic({ field: 'headers.0.name' }),
      diagnostic({ field: 'content_type' }),
    ]);

    expect(items).toHaveLength(3);
    expect(items.map((item) => item.target)).toEqual([
      { kind: 'body' },
      { kind: 'header', index: 1 },
      { kind: 'field', field: 'content_type' },
    ]);
    const grouped = groupDiagnostics(items);
    expect(grouped.body).toHaveLength(1);
    expect(grouped.headers.get(1)).toHaveLength(1);
  });

  it('sorts the body first, then headers by row, then by position', () => {
    const items = sortProblems(
      problemItems([
        diagnostic({ field: 'headers.2.value' }),
        diagnostic({ field: 'body', line: 3 }),
        diagnostic({ field: 'body', line: 1 }),
        diagnostic({ field: 'headers.0.value' }),
      ]),
    );

    expect(items.map((item) => `${item.target.kind}:${item.diagnostic.line}`)).toEqual([
      'body:1',
      'body:3',
      'header:1',
      'header:1',
    ]);
  });
});

describe('wire parsing', () => {
  it('drops malformed catalogue entries and falls back to the default delimiters', () => {
    const catalogue = parseCatalogue({
      items: [{ path: 'method', insert_text: '{{ request.method }}' }, { path: 'broken' }, 7],
    });

    expect(catalogue.delimiters).toEqual({ open: '{{', close: '}}' });
    expect(catalogue.items).toHaveLength(1);
    expect(catalogue.items[0]).toMatchObject({ path: 'method', group: 'request', type: null });
  });

  it('reads a preview defensively', () => {
    expect(parsePreview(null)).toEqual({ capture_id: null, rendered: null, diagnostics: [] });
    const result = parsePreview({
      capture_id: 'c1',
      rendered: { status: 200, content_type: 'text/plain', headers: [{ name: 'a' }], body: { data: 'x' } },
      diagnostics: [{ message: 'Bad', line: 0, severity: 'info' }, { code: 'no message' }],
    });

    expect(result.rendered?.headers).toEqual([]);
    expect(result.rendered?.body).toEqual({ encoding: 'utf8', data: 'x', truncated: false });
    expect(result.diagnostics).toEqual([
      { field: 'body', code: 'template_invalid', severity: 'error', line: 1, column: 1, length: 1, message: 'Bad' },
    ]);
  });
});

describe('preview request', () => {
  it('trims the draft and sends every header row', () => {
    const request = previewRequestOf(draftOf({}), 'c1');

    expect(request).toEqual({
      capture_id: 'c1',
      response: {
        status: 200,
        content_type: 'application/json',
        headers: [{ name: 'x-order', value: '{{ request.path }}' }],
        body: { encoding: 'utf8', data: '{}' },
      },
    });
  });

  it('falls back to status 200 while the status field is not a valid code', () => {
    expect(previewRequestOf(draftOf({ status: '' }), null).response.status).toBe(200);
    expect(previewRequestOf(draftOf({ status: '99' }), null).response.status).toBe(200);
    expect(previewRequestOf(draftOf({ status: '418' }), null).response.status).toBe(418);
  });
});

describe('small helpers', () => {
  it('picks the editor language from the content type', () => {
    expect(languageOf('application/json; charset=utf-8')).toBe('json');
    expect(languageOf('text/html')).toBe('html');
    expect(languageOf('application/atom+xml')).toBe('xml');
    expect(languageOf('text/plain')).toBe('text');
  });

  it('accepts only capture ids in the preview search parameter', () => {
    const id = '00000000-0000-4000-8000-000000000002';
    expect(validatePreviewSearch({ preview: id })).toEqual({ preview: id });
    expect(validatePreviewSearch({ preview: 'not-an-id' })).toEqual({});
    expect(validatePreviewSearch({ preview: 5 })).toEqual({});
  });

  it('reads stored panel preferences and clamps a hostile height', () => {
    expect(parsePanelPrefs(null)).toEqual(DEFAULT_PANEL_PREFS);
    expect(parsePanelPrefs('not json')).toEqual(DEFAULT_PANEL_PREFS);
    expect(parsePanelPrefs('{"hidden":true,"heightPercent":900}')).toEqual({ hidden: true, heightPercent: 70 });
    expect(parsePanelPrefs('{"heightPercent":"tall"}')).toEqual(DEFAULT_PANEL_PREFS);
  });
});
