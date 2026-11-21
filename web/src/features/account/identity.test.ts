import { describe, expect, it } from 'vitest';
import { displayNameOf, initialsOf, nameOf } from './identity';

describe('nameOf', () => {
  it.each([
    [null, null],
    ['   ', null],
    ['  Ada Lovelace ', 'Ada Lovelace'],
  ])('resolves %j to %j', (name, expected) => {
    expect(nameOf({ name })).toBe(expected);
  });
});

describe('displayNameOf', () => {
  it('falls back to the e-mail address when no name is set', () => {
    expect(displayNameOf({ name: null, email: 'dev@example.test' })).toBe('dev@example.test');
  });
});

describe('initialsOf', () => {
  it.each([
    ['Ada Lovelace', 'ada@example.test', 'AL'],
    ['Dev', 'dev@example.test', 'D'],
    ['  ', 'ada@example.test', 'A'],
    [null, 'dev@example.test', 'D'],
  ])('derives the initials of %j / %j as %j', (name, email, expected) => {
    expect(initialsOf({ name, email })).toBe(expected);
  });
});
