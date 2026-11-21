import { shellQuote } from '../export';

export const SAMPLE_DNS_LABEL = 'test';
const SAMPLE_SUBJECT = 'Test message';
const SAMPLE_BODY = 'Hello from webhooks.live';

export function emailSampleOf(address: string): string {
  return [
    'swaks',
    '--to',
    shellQuote(address),
    '--header',
    shellQuote(`Subject: ${SAMPLE_SUBJECT}`),
    '--body',
    shellQuote(SAMPLE_BODY),
  ].join(' ');
}

export function digSampleOf(name: string): string {
  return ['dig', shellQuote(`${SAMPLE_DNS_LABEL}.${name}`), 'A'].join(' ');
}
