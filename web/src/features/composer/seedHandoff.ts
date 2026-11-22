import type { ComposerSeed } from './draft';

let pending: ComposerSeed | null = null;

export function stashComposerSeed(seed: ComposerSeed): void {
  pending = seed;
}

export function peekComposerSeed(): ComposerSeed | null {
  return pending;
}

export function clearComposerSeed(): void {
  pending = null;
}
