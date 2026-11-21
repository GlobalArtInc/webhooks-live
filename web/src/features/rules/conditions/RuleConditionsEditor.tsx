import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { ConditionGroup } from '../../../components/ConditionGroup/ConditionGroup';
import { ConfirmDialog } from '../../../components/ConfirmDialog/ConfirmDialog';
import { ConflictBanner } from '../../../components/ConflictBanner/ConflictBanner';
import { SaveBar } from '../../../components/SaveBar/SaveBar';
import type { VariableEntry } from '../../../components/VariablePicker/variables';
import { DEFAULT_LIMITS } from './constants';
import { focusRowPart } from './focusPart';
import { OnMatchField } from './OnMatchField';
import styles from './RuleConditionsEditor.module.css';
import { TriggerKinds } from './TriggerKinds';
import type { ConditionCatalogue, ConditionLimits, RuleConditionsSource } from './types';
import { useConditionsDraft, type SaveConditions } from './useConditionsDraft';

export interface RuleConditionsEditorProps {
  source: RuleConditionsSource;
  catalogue: ConditionCatalogue;
  variables: readonly VariableEntry[];
  save: SaveConditions;
  onConflict?: () => void;
  readOnly?: boolean;
  limits?: ConditionLimits;
  headingLevel?: 2 | 3;
}

function isSaveShortcut(event: KeyboardEvent): boolean {
  return (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 's';
}

export function RuleConditionsEditor({
  source,
  catalogue,
  variables,
  save,
  onConflict,
  readOnly = false,
  limits = DEFAULT_LIMITS,
  headingLevel = 2,
}: RuleConditionsEditorProps) {
  const Heading = headingLevel === 2 ? 'h2' : 'h3';
  const { t } = useTranslation();
  const titleId = useId();
  const hostRef = useRef<HTMLElement>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const state = useConditionsDraft({ source, catalogue, save, onConflict, readOnly });
  const { focusRequest, status } = state;
  const locked = readOnly || !state.supported;
  const triggerMessages = [
    ...state.server.triggers.map((error) => error.message),
    ...(state.attempted && state.validation.triggersMissing ? [t('wl.conditions.triggers.required')] : []),
  ];

  useEffect(() => {
    const host = hostRef.current;
    if (focusRequest === null || host === null) {
      return;
    }
    const { target } = focusRequest;
    if (target.kind === 'row') {
      focusRowPart(host, target.id, target.part);
    } else {
      host
        .querySelector<HTMLElement>(target.kind === 'triggers' ? '[data-triggers] input' : '[data-on-match] input')
        ?.focus();
    }
  }, [focusRequest]);

  function handleKeyDown(event: KeyboardEvent): void {
    if (isSaveShortcut(event)) {
      event.preventDefault();
      void state.save();
    }
  }

  return (
    <section ref={hostRef} className={styles.editor} aria-labelledby={titleId} onKeyDown={handleKeyDown}>
      <Heading id={titleId} className={styles.title}>
        {t('wl.conditions.title')}
      </Heading>
      {state.conflict ? <ConflictBanner onReload={state.reload} onOverwrite={state.overwrite} /> : null}
      {state.supported ? null : (
        <p role="alert" className={styles.notice}>
          {t('wl.conditions.unsupported')}
        </p>
      )}
      {readOnly ? <p className={styles.notice}>{t('wl.conditions.readOnly')}</p> : null}
      <TriggerKinds
        value={state.draft.triggers}
        onChange={state.setTriggers}
        messages={triggerMessages}
        readOnly={locked}
      />
      <ConditionGroup
        root={state.draft.root}
        onChange={state.setRoot}
        catalogue={catalogue}
        triggers={state.draft.triggers}
        variables={variables}
        issues={state.issues}
        showIssues={state.attempted}
        readOnly={locked}
        limits={limits}
      />
      <OnMatchField
        value={state.draft.onMatch}
        onChange={state.setOnMatch}
        messages={state.server.onMatch.map((error) => error.message)}
        readOnly={locked}
      />
      {state.server.general.length === 0 ? null : (
        <div role="alert" className={styles.notice}>
          <p>{t('wl.conditions.general')}</p>
          <ul className={styles.errors}>
            {state.server.general.map((error, index) => (
              <li key={`${error.field}-${index}`} className={styles.error}>
                {error.message}
              </li>
            ))}
          </ul>
        </div>
      )}
      {locked ? null : (
        <SaveBar
          dirty={state.dirty}
          saving={state.saving}
          failed={state.saveFailed}
          onSave={() => {
            void state.save();
          }}
          onDiscard={() => {
            setConfirmDiscard(true);
          }}
        />
      )}
      <ConfirmDialog
        open={confirmDiscard}
        title={t('wl.conditions.discard.title')}
        body={t('wl.conditions.discard.body')}
        confirmLabel={t('wl.conditions.discard.confirm')}
        cancelLabel={t('wl.conditions.discard.cancel')}
        busy={false}
        onConfirm={() => {
          state.discard();
          setConfirmDiscard(false);
        }}
        onOpenChange={setConfirmDiscard}
      />
      <div role="status" className={styles.visuallyHidden}>
        {status === null ? null : (
          <span key={status.serial}>
            {status.kind === 'saved' ? t('wl.conditions.saved') : t('wl.conditions.problems', { count: status.count })}
          </span>
        )}
      </div>
    </section>
  );
}
