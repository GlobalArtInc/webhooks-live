import { isFormatResponse } from './guards';
import type { BodyFormatter, FormatRequest, FormatResult, FormatTask } from './types';

interface PendingJob {
  resolve: (result: FormatResult) => void;
  reject: (error: Error) => void;
}

export function createWorkerFormatter(): BodyFormatter {
  let worker: Worker | null = null;
  let nextId = 1;
  const pending = new Map<number, PendingJob>();

  const rejectAll = (error: Error): void => {
    for (const job of pending.values()) {
      job.reject(error);
    }
    pending.clear();
  };

  const stop = (): void => {
    worker?.terminate();
    worker = null;
  };

  const start = (): Worker => {
    const created = new Worker(new URL('./format.worker.ts', import.meta.url), { type: 'module' });
    created.addEventListener('message', (event: MessageEvent<unknown>) => {
      if (!isFormatResponse(event.data)) {
        return;
      }
      const job = pending.get(event.data.id);
      if (job === undefined) {
        return;
      }
      pending.delete(event.data.id);
      job.resolve(event.data.result);
    });
    created.addEventListener('error', () => {
      stop();
      rejectAll(new Error('The formatting worker failed'));
    });
    return created;
  };

  return {
    format: (task: FormatTask): Promise<FormatResult> => {
      worker ??= start();
      const id = nextId;
      nextId += 1;
      const request: FormatRequest = { id, kind: task.kind, bytes: task.bytes };
      return new Promise<FormatResult>((resolve, reject) => {
        pending.set(id, { resolve, reject });
        worker?.postMessage(request);
      });
    },
    dispose: (): void => {
      stop();
      rejectAll(new Error('The formatting worker was stopped'));
    },
  };
}
