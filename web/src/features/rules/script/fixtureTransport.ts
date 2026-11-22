import { fixtureValidate } from './fixtures';
import { SCRIPT_META_FIXTURE } from './metaFixture';
import type { ScriptTransport } from './types';

const FIXTURE_LATENCY_MS = 250;

function delayed<T>(value: () => T, signal: AbortSignal): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    if (signal.aborted) {
      reject(signal.reason);
      return;
    }
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve(value());
    }, FIXTURE_LATENCY_MS);
    function onAbort(): void {
      clearTimeout(timer);
      reject(signal.reason);
    }
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

export const fixtureScriptTransport: ScriptTransport = {
  id: 'fixture',
  loadMeta: (signal) => delayed(() => SCRIPT_META_FIXTURE, signal),
  validate: (source, signal) => delayed(() => fixtureValidate(source), signal),
};
