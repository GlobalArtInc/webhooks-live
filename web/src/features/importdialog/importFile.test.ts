import { describe, expect, it } from 'vitest';
import { EXPORTED_AT, EXPORT_TITLE, jsonExportText, ndjsonExportText } from '../../test/fixtures/exports';
import { IMPORT_MAX_BYTES, inspectSample, precheck } from './importFile';

describe('precheck', () => {
  it('refuses a ZIP export before anything is read', () => {
    expect(precheck({ name: 'captures.zip', size: 1024 })).toBe('zip');
  });

  it('accepts JSON and NDJSON names in any letter case', () => {
    expect(precheck({ name: 'Export.JSON', size: 1024 })).toBeNull();
    expect(precheck({ name: 'export.ndjson', size: 1024 })).toBeNull();
  });

  it('refuses other extensions', () => {
    expect(precheck({ name: 'notes.txt', size: 1024 })).toBe('extension');
  });

  it('refuses empty files and files above the import limit', () => {
    expect(precheck({ name: 'export.json', size: 0 })).toBe('empty');
    expect(precheck({ name: 'export.json', size: IMPORT_MAX_BYTES + 1 })).toBe('tooLarge');
    expect(precheck({ name: 'export.json', size: IMPORT_MAX_BYTES })).toBeNull();
  });
});

describe('inspectSample', () => {
  it('reads the header line of an NDJSON export', () => {
    expect(inspectSample(ndjsonExportText())).toEqual({
      ok: true,
      preview: { format: 'ndjson', version: 1, endpointTitle: EXPORT_TITLE, exportedAt: EXPORTED_AT },
    });
  });

  it('reads the header fields of a JSON document', () => {
    expect(inspectSample(jsonExportText())).toEqual({
      ok: true,
      preview: { format: 'json', version: 1, endpointTitle: EXPORT_TITLE, exportedAt: EXPORTED_AT },
    });
  });

  it('rejects text that does not start like an export', () => {
    expect(inspectSample('name,method\nalpha,GET')).toEqual({ ok: false, problem: 'notExport' });
  });

  it('rejects a sample that holds only whitespace', () => {
    expect(inspectSample(' \n\t ')).toEqual({ ok: false, problem: 'empty' });
  });
});
