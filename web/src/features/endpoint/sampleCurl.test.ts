import { describe, expect, it } from 'vitest';
import { sampleCurl, SAMPLE_BODY } from './sampleCurl';

describe('sample curl request', () => {
  it('posts a JSON test body to the capture URL with every value shell-quoted', () => {
    expect(sampleCurl('https://webhooks.example.test/7k2m9q4xw8c1d5f3h6j0n2p4')).toBe(
      `curl --request POST --url 'https://webhooks.example.test/7k2m9q4xw8c1d5f3h6j0n2p4' --header 'Content-Type: application/json' --data-raw '${SAMPLE_BODY}'`,
    );
  });

  it('keeps a single quote in the URL inside one shell word', () => {
    expect(sampleCurl("https://h.example/it's")).toContain(`--url 'https://h.example/it'\\''s'`);
  });
});
