import type { PreviewRequest, PreviewState, RenderPreview } from './types';

export const PREVIEW_DEBOUNCE_MS = 500;

export const INITIAL_PREVIEW_STATE: PreviewState = {
  phase: 'idle',
  result: null,
  resultKey: null,
  lastRendered: null,
  error: null,
};

export type PreviewListener = (state: PreviewState) => void;

function isAbort(failure: unknown): boolean {
  return failure instanceof DOMException && failure.name === 'AbortError';
}

export class PreviewRunner {
  private state: PreviewState = INITIAL_PREVIEW_STATE;
  private sequence = 0;
  private controller: AbortController | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;

  constructor(
    private readonly render: RenderPreview,
    private readonly listener: PreviewListener,
    private readonly debounceMs: number = PREVIEW_DEBOUNCE_MS,
  ) {}

  schedule(request: PreviewRequest, key: string, immediate: boolean): void {
    if (this.disposed) {
      return;
    }
    this.clearTimer();
    if (immediate) {
      this.run(request, key);
      return;
    }
    this.publish({ ...this.state, phase: 'waiting' });
    this.timer = setTimeout(() => {
      this.timer = null;
      this.run(request, key);
    }, this.debounceMs);
  }

  runNow(request: PreviewRequest, key: string): void {
    if (this.disposed) {
      return;
    }
    this.clearTimer();
    this.run(request, key);
  }

  cancel(): void {
    this.clearTimer();
    this.abortInFlight();
    this.sequence += 1;
    if (!this.disposed && this.state.phase !== 'idle') {
      this.publish({ ...this.state, phase: this.state.result === null ? 'idle' : 'done' });
    }
  }

  dispose(): void {
    this.disposed = true;
    this.clearTimer();
    this.abortInFlight();
  }

  private run(request: PreviewRequest, key: string): void {
    this.abortInFlight();
    this.sequence += 1;
    const mine = this.sequence;
    const controller = new AbortController();
    this.controller = controller;
    this.publish({ ...this.state, phase: 'rendering' });
    this.render(request, controller.signal).then(
      (result) => {
        if (mine !== this.sequence || this.disposed) {
          return;
        }
        this.controller = null;
        this.publish({
          phase: 'done',
          result,
          resultKey: key,
          lastRendered: result.rendered ?? this.state.lastRendered,
          error: null,
        });
      },
      (failure: unknown) => {
        if (mine !== this.sequence || this.disposed || isAbort(failure)) {
          return;
        }
        this.controller = null;
        this.publish({ ...this.state, phase: 'failed', error: failure });
      },
    );
  }

  private publish(next: PreviewState): void {
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
