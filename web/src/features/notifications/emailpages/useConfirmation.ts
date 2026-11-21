import { useState } from 'react';
import { ApiError } from '../../../api/client';

export type ConfirmationPhase = 'ready' | 'submitting' | 'done' | 'invalid' | 'failed';

export interface Confirmation<T> {
  phase: ConfirmationPhase;
  result: T | null;
  submit: () => Promise<void>;
}

const CLIENT_FAILURE_MIN = 400;
const SERVER_FAILURE_MIN = 500;
const RETRY_STATUS = 429;

export function phaseOfFailure(error: unknown): ConfirmationPhase {
  if (!(error instanceof ApiError)) {
    return 'failed';
  }
  if (error.status === RETRY_STATUS || error.status >= SERVER_FAILURE_MIN) {
    return 'failed';
  }
  return error.status >= CLIENT_FAILURE_MIN ? 'invalid' : 'failed';
}

export function useConfirmation<T>(token: string | null, run: (token: string) => Promise<T>): Confirmation<T> {
  const [phase, setPhase] = useState<ConfirmationPhase>(token === null ? 'invalid' : 'ready');
  const [result, setResult] = useState<T | null>(null);

  async function submit(): Promise<void> {
    if (token === null || phase === 'submitting') {
      return;
    }
    setPhase('submitting');
    try {
      const value = await run(token);
      setResult(value);
      setPhase('done');
    } catch (error: unknown) {
      setPhase(phaseOfFailure(error));
    }
  }

  return { phase, result, submit };
}
