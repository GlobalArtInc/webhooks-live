import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { useEndpoint, useUpdateEndpoint } from '../../../api/queries';
import type { Endpoint } from '../../../api/types';
import { ConflictBanner } from '../../../components/ConflictBanner/ConflictBanner';
import type { SectionProps } from '../sections';
import { DraftFooter } from '../DraftFooter';
import { useSettingsDraft } from '../useSettingsDraft';
import { DangerZone } from './DangerZone';
import styles from './GeneralSection.module.css';

export const TITLE_MAX_CHARS = 100;

export interface TitleDraft {
  title: string;
}

function titleDraftOf(endpoint: Endpoint): TitleDraft {
  return { title: endpoint.title ?? '' };
}

function serializeTitle(draft: TitleDraft): string {
  return JSON.stringify(draft);
}

function isTitleValid(draft: TitleDraft): boolean {
  return draft.title.trim().length <= TITLE_MAX_CHARS;
}

export function GeneralSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const endpointQuery = useEndpoint(endpointId);
  const endpoint = endpointQuery.data;
  const update = useUpdateEndpoint(endpointId);
  const readOnly = endpoint?.my_role === 'viewer';
  const settings = useSettingsDraft<TitleDraft, Endpoint>({
    source: endpoint,
    toDraft: titleDraftOf,
    serialize: serializeTitle,
    refresh: async () => (await endpointQuery.refetch()).data,
    write: (draft) => {
      const trimmed = draft.title.trim();
      return update.mutateAsync({ title: trimmed === '' ? null : trimmed });
    },
    validate: isTitleValid,
    readOnly,
  });

  if (endpoint === undefined || settings.draft === null) {
    return (
      <p role="status" className={styles.loading}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  const tooLong = !isTitleValid(settings.draft);
  const titleErrorId = `${titleId}-error`;

  return (
    <div className={styles.page}>
      <h2 className={styles.heading}>{t('wl.workspace.section.general')}</h2>
      {settings.conflict ? <ConflictBanner onReload={settings.reload} onOverwrite={settings.overwrite} /> : null}
      <fieldset className={styles.form} disabled={readOnly}>
        <div className={styles.field}>
          <label htmlFor={titleId} className={styles.label}>
            {t('wl.general.title.label')}
          </label>
          <input
            id={titleId}
            type="text"
            className={styles.input}
            value={settings.draft.title}
            aria-invalid={tooLong ? true : undefined}
            aria-describedby={tooLong ? titleErrorId : undefined}
            onChange={(event) => {
              settings.setDraft({ title: event.target.value });
            }}
          />
          {tooLong ? (
            <p id={titleErrorId} className={styles.error}>
              {t('wl.general.title.tooLong', { limit: TITLE_MAX_CHARS })}
            </p>
          ) : null}
        </div>
      </fieldset>
      <DangerZone endpointId={endpointId} captureCount={endpoint.capture_count} myRole={endpoint.my_role} />
      <DraftFooter status={settings} readOnly={readOnly} />
    </div>
  );
}
