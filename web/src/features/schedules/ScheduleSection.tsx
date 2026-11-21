import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import { useEndpoint, useInstance, useSchedules } from '../../api/queries';
import type { SectionProps } from '../workspace/sections';
import { useNow } from '../endpoints/useNow';
import type { ScheduleTextLimits } from './issueText';
import { ScheduleEditor } from './ScheduleEditor';
import { ScheduleList } from './ScheduleList';
import styles from './ScheduleSection.module.css';

const NEW_SELECTION = 'new';
const SECONDS_PER_MINUTE = 60;

export function ScheduleSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const instance = useInstance().data;
  const enabled = instance?.features.schedules === true;
  const schedules = useSchedules(endpointId, enabled);
  const endpoint = useEndpoint(endpointId).data;
  const now = useNow();
  const [selection, setSelection] = useState<string | null>(null);
  const heading = <h2 className={styles.heading}>{t('wl.workspace.section.schedules')}</h2>;

  if (instance === undefined) {
    return (
      <p role="status" className={styles.muted}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  if (!enabled || (schedules.error instanceof ApiError && schedules.error.code === 'feature_disabled')) {
    return (
      <div className={styles.page}>
        {heading}
        <p className={styles.muted}>{t('wl.schedules.unavailable')}</p>
      </div>
    );
  }

  if (schedules.isError) {
    return (
      <div className={styles.page}>
        {heading}
        <p role="alert" className={styles.error}>
          {t('wl.schedules.error.generic')}
        </p>
      </div>
    );
  }

  if (schedules.data === undefined) {
    return (
      <div className={styles.page}>
        {heading}
        <p role="status" className={styles.muted}>
          {t('wl.workspace.loading')}
        </p>
      </div>
    );
  }

  const items = schedules.data.items;
  const limits: ScheduleTextLimits = {
    minimumIntervalSeconds: instance.limits.schedule_min_interval_seconds,
    maxSchedules: instance.limits.schedules,
  };
  const role = endpoint?.my_role ?? null;
  const canCreate = role === 'owner';
  const readOnly = role !== 'owner' && role !== 'editor';
  const selected = items.find((item) => item.id === selection) ?? null;
  const creating = selection === NEW_SELECTION;

  return (
    <div className={styles.page}>
      {heading}
      <p className={styles.hint}>{t('wl.schedules.intro')}</p>
      <p className={styles.hint}>
        {t('wl.schedules.limits', {
          max: limits.maxSchedules,
          minutes: Math.ceil(limits.minimumIntervalSeconds / SECONDS_PER_MINUTE),
        })}
      </p>
      <div className={styles.layout}>
        <div className={styles.listColumn}>
          <div className={styles.listHeader}>
            <button
              type="button"
              className={styles.add}
              disabled={!canCreate || creating}
              onClick={() => {
                setSelection(NEW_SELECTION);
              }}
            >
              {t('wl.schedules.new')}
            </button>
          </div>
          {items.length === 0 && !creating ? <p className={styles.muted}>{t('wl.schedules.empty')}</p> : null}
          {items.length === 0 ? null : (
            <ScheduleList schedules={items} selectedId={selection} now={now} onSelect={setSelection} />
          )}
        </div>
        <div className={styles.editorColumn}>
          {creating ? (
            <ScheduleEditor
              key={NEW_SELECTION}
              endpointId={endpointId}
              schedule={null}
              readOnly={!canCreate}
              limits={limits}
              onCreated={setSelection}
              onDeleted={() => {
                setSelection(null);
              }}
            />
          ) : null}
          {selected === null ? null : (
            <ScheduleEditor
              key={selected.id}
              endpointId={endpointId}
              schedule={selected}
              readOnly={readOnly}
              limits={limits}
              onCreated={setSelection}
              onDeleted={() => {
                setSelection(null);
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
