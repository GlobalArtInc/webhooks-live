import type { EditorLanguage } from '../editor/types';

export function languageOf(contentType: string): EditorLanguage {
  const type = contentType.split(';')[0]?.trim().toLowerCase() ?? '';
  if (type.includes('json')) {
    return 'json';
  }
  if (type === 'text/html' || type === 'application/xhtml+xml') {
    return 'html';
  }
  if (type.includes('xml')) {
    return 'xml';
  }
  return 'text';
}
