import { describe, expect, it } from 'vitest';
import { jsonCaptureFixture } from '../../test/fixtures/captures';
import { dnsCaptureFixture } from '../../test/fixtures/kinds';
import { visibleSections } from './view';

describe('visibleSections', () => {
  it('shows only the DNS section for DNS captures and keeps the HTTP sections otherwise', () => {
    expect(visibleSections(dnsCaptureFixture)).toEqual(['dns']);
    expect(visibleSections(jsonCaptureFixture)[0]).toBe('summary');
    expect(visibleSections(jsonCaptureFixture)).not.toContain('dns');
  });
});
