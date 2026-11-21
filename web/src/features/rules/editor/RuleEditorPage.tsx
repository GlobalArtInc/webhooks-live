import { Link, useParams, useSearch } from '@tanstack/react-router';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useEndpoint } from '../../../api/queries';
import { useRules } from '../../../api/rules';
import type { Rule, RuleWriteBody } from '../../../api/rulesTypes';
import { StatePage } from '../../../components/StatePage/StatePage';
import { stateKindForError } from '../../../components/StatePage/stateKind';
import { inputOfRule, isScriptRuleInput, NAME_MAX_LENGTH } from './model';
import { RuleEditor } from './RuleEditor';
import styles from './RuleEditor.module.css';
import { RULE_EDITOR_ROUTE_ID } from './routeId';

const NEW_RULE_ID = 'new';
const SCRIPT_TEMPLATE = 'script';

interface InitialRule {
  input: RuleWriteBody;
  startDirty: boolean;
}

function blankInput(): RuleWriteBody {
  return { name: '', condition: {}, steps: [] };
}

function initialFor(from: string | undefined, rules: readonly Rule[], copyName: (name: string) => string): InitialRule {
  if (from === SCRIPT_TEMPLATE) {
    return { input: { name: '', condition: {}, steps: [{ kind: 'script', source: '' }] }, startDirty: false };
  }
  const source = from === undefined ? undefined : rules.find((rule) => rule.id === from);
  if (source === undefined) {
    return { input: blankInput(), startDirty: false };
  }
  return {
    input: { ...inputOfRule(source), name: copyName(source.name).slice(0, NAME_MAX_LENGTH) },
    startDirty: true,
  };
}

export function RuleEditorPage() {
  const { t } = useTranslation();
  const { id: endpointId, ruleId } = useParams({ from: RULE_EDITOR_ROUTE_ID });
  const search = useSearch({ from: RULE_EDITOR_ROUTE_ID });
  const rulesQuery = useRules(endpointId);
  const endpoint = useEndpoint(endpointId).data;
  const creating = ruleId === NEW_RULE_ID;
  const items = rulesQuery.data?.items;
  const rule = creating ? undefined : items?.find((item) => item.id === ruleId);
  const initial = useMemo<InitialRule | null>(
    () =>
      items === undefined ? null : initialFor(search.from, items, (name) => t('wl.ruleeditor.copyName', { name })),
    [items, search.from, t],
  );

  if (rulesQuery.isError) {
    return (
      <StatePage
        kind={stateKindForError(rulesQuery.error)}
        onRetry={() => {
          void rulesQuery.refetch();
        }}
      />
    );
  }

  if (items === undefined || endpoint === undefined || initial === null) {
    return (
      <p role="status" className={styles.loading}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  if (!creating && rule === undefined) {
    return (
      <StatePage
        kind="not-found"
        actions={
          <Link to="/e/$id/settings/$section" params={{ id: endpointId, section: 'rules' }} className={styles.link}>
            {t('wl.ruleeditor.back')}
          </Link>
        }
      />
    );
  }

  const base = rule === undefined ? initial.input : inputOfRule(rule);
  return (
    <RuleEditor
      key={ruleId}
      endpointId={endpointId}
      endpointUrl={endpoint.url}
      routeRuleId={ruleId}
      rule={rule ?? null}
      initial={initial.input}
      startDirty={rule === undefined && initial.startDirty}
      scriptRule={isScriptRuleInput(base)}
      canEdit={endpoint.my_role !== 'viewer'}
      search={search}
    />
  );
}
