import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { SYNTAX_TOKENS } from './syntaxPalette';

const tokensCss = readFileSync('src/styles/tokens.css', 'utf-8');

const MIN_CONTRAST = 4.5;
const BACKGROUND_TOKEN = 'color-surface-sunken';

function themeBlock(selector: string): Map<string, string> {
  const start = tokensCss.indexOf(`${selector} {`);
  const end = tokensCss.indexOf('}', start);
  const tokens = new Map<string, string>();
  for (const match of tokensCss.slice(start, end).matchAll(/--([a-z-]+):\s*(#[0-9a-f]{6});/g)) {
    const [, name, value] = match;
    if (name !== undefined && value !== undefined) {
      tokens.set(name, value);
    }
  }
  return tokens;
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((offset) => {
    const value = parseInt(hex.slice(offset, offset + 2), 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * (channels[0] ?? 0) + 0.7152 * (channels[1] ?? 0) + 0.0722 * (channels[2] ?? 0);
}

function contrast(foreground: string, background: string): number {
  const first = luminance(foreground);
  const second = luminance(background);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}

describe('syntax colour palette', () => {
  it.each([
    ['light', ':root'],
    ['dark', ":root[data-theme='dark']"],
  ])('keeps every syntax colour at 4.5:1 on the sunken surface in the %s theme', (_theme, selector) => {
    const tokens = themeBlock(selector);
    const background = tokens.get(BACKGROUND_TOKEN);
    expect(background).toBeDefined();
    for (const token of Object.values(SYNTAX_TOKENS)) {
      const colour = tokens.get(token);
      expect(colour, token).toBeDefined();
      expect(contrast(colour ?? '', background ?? ''), token).toBeGreaterThanOrEqual(MIN_CONTRAST);
    }
  });
});
