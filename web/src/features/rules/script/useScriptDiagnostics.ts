import { useEffect, useState } from 'react';
import { byteLength } from '../../editor/positions';
import { VALIDATE_DEBOUNCE_MS } from './constants';
import { httpScriptTransport } from './transport';
import type { ScriptDiagnostics, ScriptTransport, ScriptValidationState } from './types';

export interface ScriptDiagnosticsOptions {
  enabled?: boolean;
  transport?: ScriptTransport;
  debounceMs?: number;
  maxBytes?: number;
}

export interface ScriptDiagnosticsResult {
  diagnostics: ScriptDiagnostics;
  state: ScriptValidationState;
  retry: () => void;
}

interface Settled {
  source: string;
  attempt: number;
  diagnostics: ScriptDiagnostics;
  failed: boolean;
}

const NO_DIAGNOSTICS: ScriptDiagnostics = [];

export function useScriptDiagnostics(source: string, options: ScriptDiagnosticsOptions = {}): ScriptDiagnosticsResult {
  const { enabled = true, transport = httpScriptTransport, debounceMs = VALIDATE_DEBOUNCE_MS, maxBytes } = options;
  const [settled, setSettled] = useState<Settled | null>(null);
  const [attempt, setAttempt] = useState(0);

  const blank = source.trim() === '';
  const oversize = maxBytes !== undefined && byteLength(source) > maxBytes;
  const skipped = !enabled || blank || oversize;

  useEffect(() => {
    if (skipped) {
      return undefined;
    }
    const controller = new AbortController();
    const timer = setTimeout(() => {
      transport.validate(source, controller.signal).then(
        (diagnostics) => {
          if (!controller.signal.aborted) {
            setSettled({ source, attempt, diagnostics, failed: false });
          }
        },
        () => {
          if (!controller.signal.aborted) {
            setSettled({ source, attempt, diagnostics: NO_DIAGNOSTICS, failed: true });
          }
        },
      );
    }, debounceMs);
    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [skipped, source, attempt, transport, debounceMs]);

  const retry = (): void => {
    setAttempt((value) => value + 1);
  };

  if (!enabled) {
    return { diagnostics: NO_DIAGNOSTICS, state: 'idle', retry };
  }
  if (blank || oversize) {
    return { diagnostics: NO_DIAGNOSTICS, state: 'ready', retry };
  }
  if (settled === null || settled.source !== source || settled.attempt !== attempt) {
    const previous = settled !== null && !settled.failed ? settled.diagnostics : NO_DIAGNOSTICS;
    return { diagnostics: previous, state: 'pending', retry };
  }
  return { diagnostics: settled.diagnostics, state: settled.failed ? 'failed' : 'ready', retry };
}
