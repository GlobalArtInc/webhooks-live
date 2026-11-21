import { useCallback, useEffect, useRef, useState } from 'react';
import { captureExportPath, createCaptureExport } from '../../api/client';
import type { CaptureExportInput, CaptureExportQuery } from '../../api/types';
import { fallbackFileName, failureOf, fileNameFrom, type ExportFailed } from './exportRequest';

export interface IdsExportRequest {
  kind: 'ids';
  endpointId: string;
  input: CaptureExportInput;
}

export interface QueryExportRequest {
  kind: 'query';
  endpointId: string;
  query: CaptureExportQuery;
}

export type ExportRequest = IdsExportRequest | QueryExportRequest;

export type ExportPhase =
  | { kind: 'idle' }
  | { kind: 'running' }
  | { kind: 'started' }
  | { kind: 'ready'; fileName: string; size: number }
  | { kind: 'cancelled' }
  | ExportFailed;

export interface ExportJob {
  phase: ExportPhase;
  start: (request: ExportRequest) => void;
  cancel: () => void;
  downloadAgain: () => void;
  reset: () => void;
}

interface ReadyFile {
  url: string;
  fileName: string;
}

const IDLE: ExportPhase = { kind: 'idle' };

function downloadFrom(href: string, fileName: string): void {
  const anchor = document.createElement('a');
  anchor.href = href;
  anchor.download = fileName;
  document.body.append(anchor);
  anchor.click();
  anchor.remove();
}

export function useExportJob(): ExportJob {
  const [phase, setPhase] = useState<ExportPhase>(IDLE);
  const controllerRef = useRef<AbortController | null>(null);
  const readyRef = useRef<ReadyFile | null>(null);

  const release = useCallback(() => {
    controllerRef.current?.abort();
    controllerRef.current = null;
    if (readyRef.current !== null) {
      URL.revokeObjectURL(readyRef.current.url);
      readyRef.current = null;
    }
  }, []);

  useEffect(() => release, [release]);

  const start = useCallback(
    (request: ExportRequest) => {
      release();
      if (request.kind === 'query') {
        downloadFrom(captureExportPath(request.endpointId, request.query), '');
        setPhase({ kind: 'started' });
        return;
      }
      const controller = new AbortController();
      controllerRef.current = controller;
      setPhase({ kind: 'running' });
      void createCaptureExport(request.endpointId, request.input, controller.signal).then(
        (file) => {
          if (controllerRef.current !== controller) {
            return;
          }
          controllerRef.current = null;
          const fileName = fileNameFrom(file.contentDisposition, fallbackFileName(request.input.format));
          const url = URL.createObjectURL(file.blob);
          readyRef.current = { url, fileName };
          downloadFrom(url, fileName);
          setPhase({ kind: 'ready', fileName, size: file.blob.size });
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
    [release],
  );

  const cancel = useCallback(() => {
    if (controllerRef.current === null) {
      return;
    }
    release();
    setPhase({ kind: 'cancelled' });
  }, [release]);

  const downloadAgain = useCallback(() => {
    const ready = readyRef.current;
    if (ready !== null) {
      downloadFrom(ready.url, ready.fileName);
    }
  }, []);

  const reset = useCallback(() => {
    release();
    setPhase(IDLE);
  }, [release]);

  return { phase, start, cancel, downloadAgain, reset };
}
