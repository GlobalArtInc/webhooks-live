import { AUTO_RUN_DEBOUNCE_MS } from './constants';

export type RunnerPhase = 'idle' | 'waiting' | 'running' | 'done' | 'failed';

export interface RunnerState<Result> {
  phase: RunnerPhase;
  result: Result | null;
  resultKey: string | null;
  resultId: number;
  at: number;
  manual: boolean;
  error: unknown;
}

export type RunFunction<Request, Result> = (request: Request, signal: AbortSignal) => Promise<Result>;

export type RunnerListener<Result> = (state: RunnerState<Result>) => void;

export function initialRunnerState<Result>(): RunnerState<Result> {
  return { phase: 'idle', result: null, resultKey: null, resultId: 0, at: 0, manual: false, error: null };
}

function isAbort(failure: unknown): boolean {
  return failure instanceof DOMException && failure.name === 'AbortError';
}

export class LatestRunner<Request, Result> {
  private state: RunnerState<Result> = initialRunnerState<Result>();
  private sequence = 0;
  private controller: AbortController | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;

  constructor(
    private readonly execute: RunFunction<Request, Result>,
    private readonly listener: RunnerListener<Result>,
    private readonly debounceMs: number = AUTO_RUN_DEBOUNCE_MS,
  ) {}

  schedule(request: Request, key: string, immediate: boolean): void {
    if (this.disposed) {
      return;
    }
    this.clearTimer();
    if (immediate) {
      this.run(request, key, false);
      return;
    }
    this.publish({ ...this.state, phase: 'waiting' });
    this.timer = setTimeout(() => {
      this.timer = null;
      this.run(request, key, false);
    }, this.debounceMs);
  }

  runNow(request: Request, key: string): void {
    if (this.disposed) {
      return;
    }
    this.clearTimer();
    this.run(request, key, true);
  }

  cancelPending(): void {
    if (this.timer === null) {
      return;
    }
    this.clearTimer();
    if (!this.disposed && this.state.phase === 'waiting') {
      this.publish({ ...this.state, phase: this.state.result === null ? 'idle' : 'done' });
    }
  }

  cancel(): void {
    this.clearTimer();
    this.abortInFlight();
    this.sequence += 1;
    if (!this.disposed && (this.state.phase === 'waiting' || this.state.phase === 'running')) {
      this.publish({ ...this.state, phase: this.state.result === null ? 'idle' : 'done' });
    }
  }

  dispose(): void {
    this.disposed = true;
    this.clearTimer();
    this.abortInFlight();
  }

  private run(request: Request, key: string, manual: boolean): void {
    this.abortInFlight();
    this.sequence += 1;
    const mine = this.sequence;
    const controller = new AbortController();
    this.controller = controller;
    this.publish({ ...this.state, phase: 'running' });
    this.execute(request, controller.signal).then(
      (result) => {
        if (mine !== this.sequence || this.disposed) {
          return;
        }
        this.controller = null;
        this.publish({
          phase: 'done',
          result,
          resultKey: key,
          resultId: this.state.resultId + 1,
          at: Date.now(),
          manual,
          error: null,
        });
      },
      (failure: unknown) => {
        if (mine !== this.sequence || this.disposed || isAbort(failure)) {
          return;
        }
        this.controller = null;
        this.publish({ ...this.state, phase: 'failed', error: failure, manual });
      },
    );
  }

  private publish(next: RunnerState<Result>): void {
    this.state = next;
    this.listener(next);
  }

  private clearTimer(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  private abortInFlight(): void {
    this.controller?.abort();
    this.controller = null;
  }
}
