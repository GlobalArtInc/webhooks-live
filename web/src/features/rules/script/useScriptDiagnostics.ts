import { useEffect, useState } from 'react';
import { ApiError } from '../../../api/client';
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

const BUSY_STATUS = 429;
const DEFAULT_RETRY_SECONDS = 1;
const MAX_RETRY_SECONDS = 3;

function busyDelayMs(error: unknown): number | null {
  if (!(error instanceof ApiError) || error.status !== BUSY_STATUS) {
    return null;
  }
  return Math.min(error.retryAfter ?? DEFAULT_RETRY_SECONDS, MAX_RETRY_SECONDS) * 1000;
}

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
    let timer: ReturnType<typeof setTimeout> | undefined;
    const ask = (retried: boolean): void => {
      transport.validate(source, controller.signal).then(
        (diagnostics) => {
          if (!controller.signal.aborted) {
            setSettled({ source, attempt, diagnostics, failed: false });
          }
        },
        (error: unknown) => {
          if (controller.signal.aborted) {
            return;
          }
          const delay = retried ? null : busyDelayMs(error);
          if (delay !== null) {
            timer = setTimeout(() => {
              ask(true);
            }, delay);
            return;
          }
          setSettled({ source, attempt, diagnostics: NO_DIAGNOSTICS, failed: true });
        },
      );
    };
    timer = setTimeout(() => {
      ask(false);
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
