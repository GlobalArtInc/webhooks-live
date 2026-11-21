import { describe, expect, it } from 'vitest';
import { PREVIEW_MAX_BYTES } from './constants';
import { canPreview, listedParts, pickTextPart, rasterTypeOf, safeFileName } from './parts';
import type { EmailPart } from './types';

function part(overrides: Partial<EmailPart>): EmailPart {
  return {
    index: 0,
    content_type: 'application/octet-stream',
    filename: null,
    size: 10,
    disposition: null,
    is_text: false,
    ...overrides,
  };
}

describe('rasterTypeOf', () => {
  it('accepts only png, jpeg, gif and webp, ignoring case and parameters', () => {
    expect(rasterTypeOf('image/png')).toBe('png');
    expect(rasterTypeOf('IMAGE/JPEG; name="a.jpg"')).toBe('jpeg');
    expect(rasterTypeOf('image/gif')).toBe('gif');
    expect(rasterTypeOf('image/webp')).toBe('webp');
  });

  it('never accepts svg or look-alike types', () => {
    expect(rasterTypeOf('image/svg+xml')).toBeNull();
    expect(rasterTypeOf('image/png/../svg')).toBeNull();
    expect(rasterTypeOf('text/html')).toBeNull();
    expect(rasterTypeOf('')).toBeNull();
  });
});

describe('safeFileName', () => {
  it('removes path separators, bidi controls and leading dots', () => {
    expect(safeFileName('../../etc/\u202Egpj.exe', 'part-2')).toBe('_.._etc_gpj.exe');
    expect(safeFileName('  ..hidden.txt ', 'part-1')).toBe('hidden.txt');
    expect(safeFileName('a\u0000b:c*.txt', 'part-1')).toBe('ab_c_.txt');
  });

  it('replaces invisible filler letters so an extension cannot be disguised', () => {
    expect(safeFileName('invoice.pdf\u3164.exe', 'part-1')).toBe('invoice.pdf_.exe');
    expect(safeFileName('a\u2800b\u00A0c', 'part-1')).toBe('a_b c');
  });

  it('falls back when nothing usable is left and avoids reserved device names', () => {
    expect(safeFileName(null, 'part-4')).toBe('part-4');
    expect(safeFileName('\u202E\u202C', 'part-4')).toBe('part-4');
    expect(safeFileName('...', 'part-4')).toBe('part-4');
    expect(safeFileName('NUL.txt', 'part-4')).toBe('_NUL.txt');
  });

  it('shortens long names and keeps the extension', () => {
    const name = safeFileName(`${'a'.repeat(400)}.pdf`, 'part-1');
    expect(name.length).toBeLessThanOrEqual(120);
    expect(name.endsWith('.pdf')).toBe(true);
  });

  it('does not split a surrogate pair when it shortens a name', () => {
    const name = safeFileName(`${'a'.repeat(100)}${'\u{1F600}'.repeat(30)}.pdf`, 'part-1');
    expect(name.endsWith('.pdf')).toBe(true);
    expect(/\p{Cs}/u.test(name)).toBe(false);
    expect(name.length).toBeLessThanOrEqual(120);
  });
});

describe('part selection', () => {
  const body = part({ index: 0, content_type: 'text/plain; charset=utf-8', is_text: true });
  const html = part({ index: 1, content_type: 'text/html', is_text: true });
  const named = part({ index: 2, content_type: 'text/plain', filename: 'notes.txt', is_text: true });

  it('picks the first unnamed inline plain-text part as the body', () => {
    expect(pickTextPart([html, named, body])).toBe(body);
    expect(pickTextPart([html, named])).toBeNull();
    expect(pickTextPart([part({ content_type: 'text/plain', is_text: true, disposition: 'attachment' })])).toBeNull();
  });

  it('lists every other part for download', () => {
    expect(listedParts([body, html, named], body)).toEqual([html, named]);
  });

  it('previews raster images only up to the size limit', () => {
    expect(canPreview(part({ content_type: 'image/png', size: PREVIEW_MAX_BYTES }), PREVIEW_MAX_BYTES)).toBe(true);
    expect(canPreview(part({ content_type: 'image/png', size: PREVIEW_MAX_BYTES + 1 }), PREVIEW_MAX_BYTES)).toBe(false);
    expect(canPreview(part({ content_type: 'image/png', size: 0 }), PREVIEW_MAX_BYTES)).toBe(false);
    expect(canPreview(part({ content_type: 'image/svg+xml' }), PREVIEW_MAX_BYTES)).toBe(false);
  });
});
