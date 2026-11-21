import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { BinaryView } from './BinaryView';
import styles from './BodyViewer.module.css';
import { decodeText, formatSize, pickLonger, toBytes } from './bytes';
import { classifyBody } from './classify';
import { CodeViewer } from './CodeViewer';
import { LARGE_BODY_LIMIT, PREVIEW_LIMIT } from './constants';
import { formatDocument } from './format';
import { ImagePreview } from './ImagePreview';
import type {
  BodyClass,
  BodyLoader,
  BodyViewMode,
  BodyViewerProps,
  Bytes,
  CodeLanguage,
  CodeViewerHandle,
  FormatKind,
  FormatResult,
  ImageType,
} from './types';
import { useAsyncValue } from './useAsyncValue';
import { ViewerControls } from './ViewerControls';
import { createWorkerFormatter } from './workerFormatter';

interface LoadKey {
  attempt: number;
}

interface IdlePretty {
  status: 'idle';
}

interface WorkingPretty {
  status: 'working';
}

interface DonePretty {
  status: 'done';
  text: string;
}

interface FailedPretty {
  status: 'failed';
  message: string;
}

type PrettyState = IdlePretty | WorkingPretty | DonePretty | FailedPretty;

interface CodePlan {
  type: 'code';
  language: CodeLanguage;
  formatKind: FormatKind | null;
}

interface BinaryPlan {
  type: 'binary';
}

interface ImagePlan {
  type: 'image';
  image: ImageType;
}

type ViewPlan = CodePlan | BinaryPlan | ImagePlan;

const IDLE_PRETTY: PrettyState = { status: 'idle' };
const EDITOR_LABEL = 'Request body';

async function fetchBody(loader: BodyLoader | undefined, limit: number | null): Promise<Bytes> {
  if (loader === undefined) {
    throw new Error('No body loader was provided');
  }
  return loader(limit);
}

function describeError(error: unknown): string {
  return error instanceof Error ? error.message : 'Unknown error';
}

function textPlan(kind: 'json' | 'xml' | 'text' | 'html', large: boolean): CodePlan {
  const formatKind = kind === 'json' || kind === 'xml' ? kind : null;
  if (large || kind === 'text') {
    return { type: 'code', language: 'plain', formatKind };
  }
  return { type: 'code', language: kind, formatKind };
}

function planFor(cls: BodyClass, complete: boolean): ViewPlan {
  if (cls.kind === 'image') {
    return { type: 'image', image: cls.image };
  }
  if (cls.kind === 'binary') {
    return { type: 'binary' };
  }
  if (cls.kind === 'large') {
    if (cls.hint.kind === 'image' && complete) {
      return { type: 'image', image: cls.hint.image };
    }
    if (cls.hint.kind === 'image' || cls.hint.kind === 'binary') {
      return { type: 'binary' };
    }
    return textPlan(cls.hint.kind, true);
  }
  return textPlan(cls.kind, false);
}

function prettyFromResult(result: FormatResult): PrettyState {
  return result.ok ? { status: 'done', text: result.text } : { status: 'failed', message: result.error.message };
}

function BodyViewerContent({
  body,
  loadBody,
  contentType,
  size,
  mode,
  wrap,
  search,
  onModeChange,
  onWrapChange,
  onSearchChange,
  onDownloadRaw,
  formatter,
}: BodyViewerProps) {
  const viewerRef = useRef<CodeViewerHandle>(null);
  const jobRef = useRef(0);
  const [ownFormatter] = useState(createWorkerFormatter);
  const [wantFull, setWantFull] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [pretty, setPretty] = useState<PrettyState>(IDLE_PRETTY);

  const supplied = useMemo(() => (body === undefined ? null : toBytes(body)), [body]);
  const large = size > LARGE_BODY_LIMIT;
  const prefixNeeded = large ? PREVIEW_LIMIT : size;
  const canLoad = loadBody !== undefined;
  const prefixSupplied = supplied !== null && supplied.length >= prefixNeeded;
  const fullSupplied = supplied !== null && supplied.length >= size;

  const prefixKey = useMemo<LoadKey | null>(
    () => (canLoad && !prefixSupplied ? { attempt } : null),
    [canLoad, prefixSupplied, attempt],
  );
  const fullKey = useMemo<LoadKey | null>(
    () => (large && wantFull && canLoad && !fullSupplied ? { attempt } : null),
    [large, wantFull, canLoad, fullSupplied, attempt],
  );
  const prefixLoad = useAsyncValue(prefixKey, () => fetchBody(loadBody, large ? PREVIEW_LIMIT : null));
  const fullLoad = useAsyncValue(fullKey, () => fetchBody(loadBody, null));

  const loadedPrefix = prefixLoad.status === 'done' ? prefixLoad.value : null;
  const prefixSource = pickLonger(supplied, loadedPrefix);
  const prefixReady = prefixSource !== null && (prefixSource.length >= prefixNeeded || prefixLoad.status === 'done');
  const fullBytes = useMemo<Bytes | null>(() => {
    if (!large || !wantFull) {
      return null;
    }
    if (fullSupplied) {
      return supplied;
    }
    return fullLoad.status === 'done' ? fullLoad.value : null;
  }, [large, wantFull, fullSupplied, supplied, fullLoad]);

  const shown = useMemo<Bytes | null>(() => {
    if (fullBytes !== null) {
      return fullBytes;
    }
    if (!prefixReady || prefixSource === null) {
      return null;
    }
    return prefixSource.length > prefixNeeded ? prefixSource.subarray(0, prefixNeeded) : prefixSource;
  }, [fullBytes, prefixReady, prefixSource, prefixNeeded]);

  const complete = shown !== null && shown.length >= size;
  const plan = useMemo<ViewPlan | null>(() => {
    if (shown === null) {
      return null;
    }
    return planFor(classifyBody(shown, contentType, size), complete);
  }, [shown, contentType, size, complete]);

  const codePlan = plan !== null && plan.type === 'code' ? plan : null;
  const formatKind = codePlan === null ? null : codePlan.formatKind;
  const rawText = useMemo(
    () => (shown !== null && codePlan !== null ? decodeText(shown, shown.length < size) : ''),
    [shown, codePlan, size],
  );
  const smallFormatted = useMemo<FormatResult | null>(
    () => (!large && mode === 'pretty' && formatKind !== null ? formatDocument(formatKind, rawText) : null),
    [large, mode, formatKind, rawText],
  );

  useEffect(
    () => () => {
      jobRef.current += 1;
      ownFormatter.dispose();
    },
    [ownFormatter],
  );

  const startPretty = (): void => {
    if (fullBytes === null || formatKind === null || pretty.status === 'working' || pretty.status === 'done') {
      return;
    }
    jobRef.current += 1;
    const job = jobRef.current;
    setPretty({ status: 'working' });
    (formatter ?? ownFormatter).format({ kind: formatKind, bytes: fullBytes }).then(
      (result) => {
        if (jobRef.current === job) {
          setPretty(prettyFromResult(result));
        }
      },
      (error: unknown) => {
        if (jobRef.current === job) {
          setPretty({ status: 'failed', message: describeError(error) });
        }
      },
    );
  };

  const handleModeChange = (next: BodyViewMode): void => {
    onModeChange(next);
    if (next === 'pretty' && large) {
      startPretty();
    }
  };

  let displayText = rawText;
  let notice: string | null = null;
  if (mode === 'pretty') {
    if (large) {
      if (pretty.status === 'done') {
        displayText = pretty.text;
      } else if (pretty.status === 'failed') {
        notice = pretty.message;
      }
    } else if (smallFormatted !== null) {
      if (smallFormatted.ok) {
        displayText = smallFormatted.text;
      } else {
        notice = smallFormatted.error.message;
      }
    }
  }

  const shownMode: BodyViewMode = large ? (mode === 'pretty' && pretty.status !== 'idle' ? 'pretty' : 'raw') : mode;
  const loadFailure =
    prefixLoad.status === 'failed' ? prefixLoad.message : fullLoad.status === 'failed' ? fullLoad.message : null;
  const loadingFull = wantFull && fullKey !== null && fullLoad.status === 'pending';
  const gated = large && plan !== null && !wantFull && (plan.type === 'code' || !complete);

  let content: ReactNode = null;
  if (plan === null) {
    if (loadFailure === null) {
      content = (
        <p className={styles.status} role="status">
          {canLoad ? 'Loading body' : 'The body is not available'}
        </p>
      );
    }
  } else if (plan.type === 'image') {
    content = shown === null ? null : <ImagePreview bytes={shown} image={plan.image} />;
  } else if (plan.type === 'binary') {
    content =
      shown === null ? null : <BinaryView bytes={shown} contentType={contentType} size={size} complete={complete} />;
  } else {
    content = (
      <CodeViewer
        ref={viewerRef}
        text={displayText}
        language={plan.language}
        wrap={wrap}
        search={search}
        label={EDITOR_LABEL}
      />
    );
  }

  return (
    <section className={styles.viewer} aria-label="Request body">
      <ViewerControls
        showMode={formatKind !== null}
        mode={shownMode}
        prettyDisabled={large && fullBytes === null}
        onModeChange={handleModeChange}
        showWrap={codePlan !== null}
        wrap={wrap}
        onWrapChange={onWrapChange}
        showSearch={codePlan !== null}
        search={search}
        onSearchChange={onSearchChange}
        onFindNext={() => {
          viewerRef.current?.findNext();
        }}
        onFindPrevious={() => {
          viewerRef.current?.findPrevious();
        }}
        onDownloadRaw={onDownloadRaw}
      />
      {gated ? (
        <div className={styles.notice}>
          <span>
            Showing the first {formatSize(PREVIEW_LIMIT)} of {formatSize(size)}.
          </span>
          <button
            type="button"
            className={styles.action}
            onClick={() => {
              setWantFull(true);
            }}
          >
            Load full body ({formatSize(size)})
          </button>
        </div>
      ) : null}
      {loadingFull ? (
        <p className={styles.status} role="status">
          Loading full body
        </p>
      ) : null}
      {pretty.status === 'working' ? (
        <p className={styles.status} role="status">
          Formatting in a background worker
        </p>
      ) : null}
      {notice !== null ? (
        <p className={styles.status} role="status">
          {notice}
        </p>
      ) : null}
      {loadFailure !== null ? (
        <div className={styles.notice} role="alert">
          <span>Could not load the body: {loadFailure}</span>
          <button
            type="button"
            className={styles.action}
            onClick={() => {
              setAttempt(attempt + 1);
            }}
          >
            Retry
          </button>
        </div>
      ) : null}
      {content}
    </section>
  );
}

export function BodyViewer(props: BodyViewerProps) {
  return <BodyViewerContent key={props.bodyId} {...props} />;
}
