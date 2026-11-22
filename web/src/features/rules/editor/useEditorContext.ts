import { useMemo } from 'react';
import { offersNotifications, offersSecrets } from '../../../api/capabilities';
import { useInstance } from '../../../api/queries';
import type { VariableEntry } from '../../../components/VariablePicker/variables';
import { DEFAULT_DELIMITERS } from '../../editor/constants';
import type { TemplateDelimiters } from '../../editor/types';
import { useChannels, useSecrets } from '../../notifications/notificationsQueries';
import { toVariableEntries } from '../../templating/catalogue';
import { useVariableCatalogue } from '../../templating/hooks';
import { findEntry, producedVariables } from '../steps/model';
import type { RuleStep as StepDraft, StepEditorContext } from '../steps/types';
import type { RuleCatalogue } from './types';

export interface EditorContextData {
  variables: readonly VariableEntry[];
  delimiters: TemplateDelimiters;
  context: StepEditorContext;
}

const NO_NAMES: readonly string[] = [];

export function useEditorContext(
  endpointId: string,
  captureId: string | null,
  steps: readonly StepDraft[],
  catalogue: RuleCatalogue,
): EditorContextData {
  const variableQuery = useVariableCatalogue(endpointId, captureId, true);
  const instance = useInstance().data;
  const channelQuery = useChannels(endpointId, offersNotifications(instance));
  const secretQuery = useSecrets(endpointId, offersSecrets(instance));
  const served = variableQuery.data;

  const variables = useMemo(() => {
    const entries = served === undefined ? [] : toVariableEntries(served.items);
    const open = served?.delimiters.open ?? DEFAULT_DELIMITERS.open;
    const close = served?.delimiters.close ?? DEFAULT_DELIMITERS.close;
    const known = new Set(entries.map((entry) => entry.path));
    for (const step of steps) {
      const entry = findEntry(catalogue.steps, step.kind);
      for (const path of entry === undefined ? [] : producedVariables(entry, step)) {
        if (!known.has(path)) {
          known.add(path);
          entries.push({ path, insertText: `${open} ${path} ${close}`, group: 'rules' });
        }
      }
    }
    return entries;
  }, [served, steps, catalogue]);

  const channels = channelQuery.data?.items;
  const secrets = secretQuery.data?.items;
  const context = useMemo<StepEditorContext>(
    () => ({
      variables,
      secrets: secrets === undefined ? NO_NAMES : secrets.map((secret) => secret.name),
      channels: channels === undefined ? [] : channels.map((channel) => ({ id: channel.id, label: channel.name })),
      scriptEditor: true,
      guards: false,
    }),
    [variables, secrets, channels],
  );

  return { variables, delimiters: served?.delimiters ?? DEFAULT_DELIMITERS, context };
}
