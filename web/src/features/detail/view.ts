import type { Body, Capture, NameValue } from '../../api/types';
import type { BodyContent, Bytes } from '../body';
import type { DetailSectionId } from './types';

const QUERY_SEPARATOR = '&';
const VALUE_SEPARATOR = '=';

function decodeBase64(data: string): Bytes {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

export function visibleSections(capture: Capture): DetailSectionId[] {
  const sections: DetailSectionId[] = ['summary'];
  if (capture.forwards.length > 0) {
    sections.push('forward');
  }
  if (capture.query.length > 0) {
    sections.push('query');
  }
  if (capture.form !== null) {
    sections.push('form');
  }
  if (capture.body_size > 0) {
    sections.push('body');
  }
  sections.push('headers');
  return sections;
}

export function parseRawQuery(queryString: string): NameValue[] {
  return queryString
    .split(QUERY_SEPARATOR)
    .filter((part) => part !== '')
    .map((part) => {
      const index = part.indexOf(VALUE_SEPARATOR);
      return index === -1
        ? { name: part, value: '' }
        : { name: part.slice(0, index), value: part.slice(index + VALUE_SEPARATOR.length) };
    });
}

export function toBodyContent(body: Body): BodyContent {
  if (body.encoding === 'utf8') {
    return { type: 'text', text: body.data };
  }
  return { type: 'bytes', bytes: decodeBase64(body.data) };
}
