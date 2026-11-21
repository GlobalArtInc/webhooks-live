import { useQueryClient, type QueryClient } from '@tanstack/react-query';
import { useCallback, useEffect, useRef, useState } from 'react';
import { queryKeys } from '../../api/queries';
import { rememberEndpoint } from '../endpoints/store';
import type { ImportFormat } from './importFile';
import { failureOf, sendImport, type ImportFailed, type ImportOutcome } from './importRequest';

export interface ImportRequest {
  endpointId: string | null;
  file: File;
  format: ImportFormat;
  keepTimes: boolean;
}

export interface ImportUploading {
  kind: 'uploading';
  progress: number;
}

export interface ImportDone {
  kind: 'done';
  outcome: ImportOutcome;
  created: boolean;
}

export type ImportPhase = { kind: 'idle' } | ImportUploading | ImportDone | { kind: 'cancelled' } | ImportFailed;

export interface ImportJob {
  phase: ImportPhase;
  start: (request: ImportRequest) => void;
  cancel: () => void;
  reset: () => void;
}

const IDLE: ImportPhase = { kind: 'idle' };

function refreshAfter(queryClient: QueryClient, endpointId: string | null): void {
  if (endpointId === null) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.endpointLists() });
    return;
  }
  void queryClient.invalidateQueries({ queryKey: queryKeys.captures(endpointId) });
  void queryClient.invalidateQueries({ queryKey: queryKeys.endpoint(endpointId) });
}

export function useImportJob(): ImportJob {
  const queryClient = useQueryClient();
  const [phase, setPhase] = useState<ImportPhase>(IDLE);
  const controllerRef = useRef<AbortController | null>(null);

  const release = useCallback(() => {
    controllerRef.current?.abort();
    controllerRef.current = null;
  }, []);

  useEffect(() => release, [release]);

  const start = useCallback(
    (request: ImportRequest) => {
      release();
      const controller = new AbortController();
      controllerRef.current = controller;
      setPhase({ kind: 'uploading', progress: 0 });
      void sendImport({
        endpointId: request.endpointId,
        keepTimes: request.keepTimes,
        format: request.format,
        file: request.file,
        signal: controller.signal,
        onProgress: (progress) => {
          if (controllerRef.current === controller) {
            setPhase({ kind: 'uploading', progress });
          }
        },
      }).then(
        (outcome) => {
          if (controllerRef.current !== controller) {
            return;
          }
          controllerRef.current = null;
          refreshAfter(queryClient, request.endpointId);
          const created = request.endpointId === null;
          if (created && outcome.endpointId !== null) {
            rememberEndpoint(outcome.endpointId);
          }
          setPhase({ kind: 'done', outcome, created });
        },
        (error: unknown) => {
          if (controllerRef.current !== controller) {
            return;
          }
          controllerRef.current = null;
          setPhase(failureOf(error));
        },
      );
    },
    [queryClient, release],
  );

  const cancel = useCallback(() => {
    if (controllerRef.current === null) {
      return;
    }
    release();
    setPhase({ kind: 'cancelled' });
  }, [release]);

  const reset = useCallback(() => {
    release();
    setPhase(IDLE);
  }, [release]);

  return { phase, start, cancel, reset };
}
