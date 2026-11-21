import { describe, expect, it } from 'vitest';
import { buildForwardCommand, isValidPrefix, parsePort, shellWord } from './command';

const ENDPOINT_URL = 'https://webhooks.example.test/7k2m9q4xw8c1d5f3h6j0n2p4';

describe('parsePort', () => {
  it('accepts the bounds of the port range and trims spaces', () => {
    expect(parsePort('1')).toBe(1);
    expect(parsePort(' 65535 ')).toBe(65535);
  });

  it('rejects zero, values above the range, and anything that is not digits', () => {
    expect(parsePort('0')).toBeNull();
    expect(parsePort('65536')).toBeNull();
    expect(parsePort('')).toBeNull();
    expect(parsePort('30x0')).toBeNull();
  });
});

describe('isValidPrefix', () => {
  it('accepts an empty prefix and a slash-led path made of safe characters', () => {
    expect(isValidPrefix('')).toBe(true);
    expect(isValidPrefix('/hooks/v1')).toBe(true);
  });

  it('rejects a prefix without a leading slash or with characters that need shell quoting', () => {
    expect(isValidPrefix('hooks')).toBe(false);
    expect(isValidPrefix('/a b')).toBe(false);
    expect(isValidPrefix('/x?y=1')).toBe(false);
  });
});

describe('buildForwardCommand', () => {
  it('builds the default command with the capture URL and the local target', () => {
    const command = buildForwardCommand({
      endpointUrl: ENDPOINT_URL,
      port: 3000,
      prefix: '',
      start: 'resume',
      dryRun: false,
    });

    expect(command).toBe(`webhooks-live-cli forward ${ENDPOINT_URL} --to http://localhost:3000`);
  });

  it('adds the path prefix to the target and the start and dry run flags', () => {
    const command = buildForwardCommand({
      endpointUrl: ENDPOINT_URL,
      port: 8080,
      prefix: '/hooks',
      start: 'all',
      dryRun: true,
    });

    expect(command).toBe(
      `webhooks-live-cli forward ${ENDPOINT_URL} --to http://localhost:8080/hooks --from all --dry-run`,
    );
  });

  it('never adds a token to the command', () => {
    const command = buildForwardCommand({
      endpointUrl: ENDPOINT_URL,
      port: 3000,
      prefix: '',
      start: 'now',
      dryRun: false,
    });

    expect(command).not.toContain('--token');
    expect(command).not.toContain('WEBHOOKS_LIVE_TOKEN');
  });
});

describe('shellWord', () => {
  it('quotes a value that contains shell metacharacters', () => {
    expect(shellWord("https://example.test/a;rm 'x'")).toBe("'https://example.test/a;rm '\\''x'\\'''");
  });
});
