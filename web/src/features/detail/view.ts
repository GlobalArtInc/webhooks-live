import type { Body, Capture, CaptureKind, NameValue } from '../../api/types';
import type { BodyContent, Bytes } from '../body';
import { captureKindOf } from '../kinds/kinds';
import type { DetailSectionId } from './types';

const QUERY_SEPARATOR = '&';
const VALUE_SEPARATOR = '=';

const KIND_SECTIONS: Partial<Readonly<Record<CaptureKind, readonly DetailSectionId[]>>> = {
  dns: ['dns'],
};

function decodeBase64(data: string): Bytes {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

export function visibleSections(capture: Capture): DetailSectionId[] {
  const kindSections = KIND_SECTIONS[captureKindOf(capture)];
  if (kindSections !== undefined) {
    return [...kindSections];
  }
  const sections: DetailSectionId[] = ['summary'];
  if (capture.forwards.length > 0) {
    sections.push('forward');
  }
  if ((capture.rule_runs ?? []).length > 0) {
    sections.push('response', 'rules');
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
