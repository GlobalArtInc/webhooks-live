import { shellQuote } from '../export';

export const SAMPLE_BODY = '{"event":"test","source":"webhooks.live"}';

export function sampleCurl(captureUrl: string): string {
  return [
    'curl',
    '--request',
    'POST',
    '--url',
    shellQuote(captureUrl),
    '--header',
    shellQuote('Content-Type: application/json'),
    '--data-binary',
    shellQuote(SAMPLE_BODY),
  ].join(' ');
}
