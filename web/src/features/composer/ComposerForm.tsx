import { useRef, useState, type KeyboardEvent } from 'react';
import { validateTarget } from '../forwarding/ruleDraft';
import { buildSendRequest, redactedRowIds, type ComposerDraft } from './draft';
import { useComposerMessages } from './messages';
import { RequestEditor } from './RequestEditor';
import { ResponseView } from './ResponseView';
import { failureOf, failureOfResult, type SendFailure, type UrlIssue } from './sendErrors';
import { useSendComposerRequest } from './useComposer';
import styles from './ComposerForm.module.css';

export interface ComposerFormProps {
  endpointId: string | null;
  sourceCaptureId: string | null;
  initialDraft: ComposerDraft;
}

function urlIssueOf(failure: SendFailure | null): UrlIssue | null {
  return failure?.kind === 'url' ? failure.issue : null;
}

export function ComposerForm({ endpointId, sourceCaptureId, initialDraft }: ComposerFormProps) {
  const { failureText } = useComposerMessages();
  const [draft, setDraft] = useState(initialDraft);
  const [attempted, setAttempted] = useState(false);
  const controllerRef = useRef<AbortController | null>(null);
  const request = useSendComposerRequest();
  const redactedIds = redactedRowIds(draft.headers);
  const failure = request.isError ? failureOf(request.error) : null;
  const result = request.data ?? null;
  const unreachable = result === null ? null : failureOfResult(result);
  const clientIssue = attempted ? validateTarget(draft.url) : null;
  const urlIssue = clientIssue ?? urlIssueOf(failure) ?? urlIssueOf(unreachable);
  const notice = failure !== null && failure.kind !== 'url' ? failure : null;

  function edit(next: ComposerDraft): void {
    if (next.url !== draft.url) {
      request.reset();
    }
    setDraft(next);
  }

  function submit(): void {
    setAttempted(true);
    if (request.isPending || validateTarget(draft.url) !== null || redactedIds.length > 0) {
      return;
    }
    const controller = new AbortController();
    controllerRef.current = controller;
    request.mutate({
      input: buildSendRequest(draft, { endpointId, sourceCaptureId }),
      signal: controller.signal,
    });
  }

  function cancel(): void {
    controllerRef.current?.abort();
  }

  function submitFromKeyboard(event: KeyboardEvent<HTMLDivElement>): void {
    if (event.key !== 'Enter' || !(event.metaKey || event.ctrlKey)) {
      return;
    }
    const inEditor = event.target instanceof Element && event.target.closest('.cm-editor') !== null;
    if (!inEditor) {
      event.preventDefault();
      submit();
    }
  }

  return (
    <div className={styles.form} onKeyDown={submitFromKeyboard}>
      <RequestEditor
        draft={draft}
        onChange={edit}
        urlIssue={urlIssue}
        redactedIds={redactedIds}
        sending={request.isPending}
        onSend={submit}
        onCancel={cancel}
      />
      {notice === null ? null : (
        <p role="alert" className={styles.notice}>
          {failureText(notice)}
        </p>
      )}
      <ResponseView result={result} sending={request.isPending} />
    </div>
  );
}
