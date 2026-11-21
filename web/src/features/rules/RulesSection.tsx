import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type Announcements,
  type DragEndEvent,
} from '@dnd-kit/core';
import { SortableContext, verticalListSortingStrategy } from '@dnd-kit/sortable';
import { Link, useNavigate, useSearch } from '@tanstack/react-router';
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useEndpoint } from '../../api/queries';
import { useDeleteRule, useReorderRules, useRules, useSetRuleEnabled } from '../../api/rules';
import type { Rule } from '../../api/rulesTypes';
import type { CapturePick } from '../../components/CapturePicker/CapturePicker';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { StatePage } from '../../components/StatePage/StatePage';
import { stateKindForError } from '../../components/StatePage/stateKind';
import type { SectionProps } from '../workspace/sections';
import { moveItem, targetIndex, type MoveControl } from './order';
import { RuleCard } from './RuleCard';
import { RulesToast } from './RulesToast';
import styles from './RulesSection.module.css';

type NoticeTone = 'info' | 'danger';

interface RuleUndo {
  ruleId: string;
  enabled: boolean;
}

interface RulesNotice {
  id: number;
  message: string;
  tone: NoticeTone;
  undo: RuleUndo | null;
}

const NEW_RULE_KEY = 'section:new';
const SECTION_ROUTE_ID = '/shell/e/$id/settings/$section';
const SECTION_ROUTE = '/e/$id/settings/$section';
const RuleTester = lazy(() => import('./tester/RuleTester').then((module) => ({ default: module.RuleTester })));
const POINTER_DISTANCE_PX = 6;
const NO_RULES: Rule[] = [];

export function RulesSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const endpoint = useEndpoint(endpointId).data;
  const rulesQuery = useRules(endpointId);
  const setEnabled = useSetRuleEnabled(endpointId);
  const reorder = useReorderRules(endpointId);
  const remove = useDeleteRule(endpointId);
  const [announcement, setAnnouncement] = useState('');
  const [notice, setNotice] = useState<RulesNotice | null>(null);
  const [pendingDelete, setPendingDelete] = useState<Rule | null>(null);
  const search = useSearch({ from: SECTION_ROUTE_ID });
  const navigate = useNavigate();
  const rootRef = useRef<HTMLDivElement>(null);
  const focusKeys = useRef<string[]>([]);
  const noticeCount = useRef(0);
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: POINTER_DISTANCE_PX } }));
  const dismissNotice = useCallback(() => {
    setNotice(null);
  }, []);

  const testing = search.test === 'all';
  const pinned = search.capture ?? null;
  const pick = useMemo<CapturePick>(
    () => (pinned === null ? { mode: 'latest' } : { mode: 'pinned', captureId: pinned }),
    [pinned],
  );
  const items = rulesQuery.data?.items ?? NO_RULES;
  const total = items.length;
  const readOnly = endpoint?.my_role === 'viewer';
  const undo = notice?.undo ?? null;

  useEffect(() => {
    const keys = focusKeys.current;
    if (keys.length === 0) {
      return;
    }
    focusKeys.current = [];
    for (const key of keys) {
      const element = rootRef.current?.querySelector<HTMLElement>(`[data-focus-key="${key}"]`) ?? null;
      if (element !== null && !(element instanceof HTMLButtonElement && element.disabled)) {
        element.focus();
        return;
      }
    }
  }, [items]);

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

  if (rulesQuery.data === undefined) {
    return (
      <p role="status" className={styles.loading}>
        {t('wl.workspace.loading')}
      </p>
    );
  }

  function setTestAll(open: boolean, captureId: string | null): void {
    void navigate({
      to: SECTION_ROUTE,
      params: { id: endpointId, section: 'rules' },
      search: open ? { test: 'all', capture: captureId ?? undefined } : {},
      replace: true,
    });
  }

  function notify(message: string, tone: NoticeTone, undoAction: RuleUndo | null): void {
    noticeCount.current += 1;
    setNotice({ id: noticeCount.current, message, tone, undo: undoAction });
  }

  function isBusy(ruleId: string): boolean {
    return (
      reorder.isPending ||
      (setEnabled.isPending && setEnabled.variables?.ruleId === ruleId) ||
      (remove.isPending && remove.variables === ruleId)
    );
  }

  function positionOf(id: string): number {
    return items.findIndex((rule) => rule.id === id) + 1;
  }

  function nameOf(id: string): string {
    return items.find((rule) => rule.id === id)?.name ?? '';
  }

  function toggleRule(rule: Rule): void {
    const enabled = !rule.enabled;
    const previous: RuleUndo = { ruleId: rule.id, enabled: rule.enabled };
    setEnabled.mutate(
      { ruleId: rule.id, enabled },
      {
        onSuccess: () => {
          if (enabled) {
            notify(t('wl.rules.toggle.on', { name: rule.name }), 'info', previous);
          } else {
            notify(t('wl.rules.toggle.off', { name: rule.name }), 'info', previous);
          }
        },
        onError: () => {
          notify(t('wl.rules.error.toggle'), 'danger', null);
        },
      },
    );
  }

  function undoToggle(action: RuleUndo): void {
    setNotice(null);
    setEnabled.mutate(
      { ruleId: action.ruleId, enabled: action.enabled },
      {
        onError: () => {
          notify(t('wl.rules.error.toggle'), 'danger', null);
        },
      },
    );
  }

  function placeRule(ruleId: string, to: number, keys: string[]): void {
    const from = items.findIndex((rule) => rule.id === ruleId);
    const rule = items[from];
    if (reorder.isPending || rule === undefined || to === from || to < 0 || to >= total) {
      return;
    }
    const nextIds = moveItem(
      items.map((item) => item.id),
      from,
      to,
    );
    focusKeys.current = keys;
    setAnnouncement(t('wl.rules.moved', { name: rule.name, position: to + 1, total }));
    reorder.mutate(nextIds, {
      onError: () => {
        notify(t('wl.rules.error.order'), 'danger', null);
      },
    });
  }

  function moveRule(ruleId: string, control: MoveControl): void {
    const from = items.findIndex((rule) => rule.id === ruleId);
    if (from < 0) {
      return;
    }
    placeRule(ruleId, targetIndex(from, control, total), [`${ruleId}:${control}`, `${ruleId}:name`]);
  }

  function handleDragEnd(event: DragEndEvent): void {
    const { active, over } = event;
    if (over === null) {
      return;
    }
    const activeId = String(active.id);
    placeRule(
      activeId,
      items.findIndex((rule) => rule.id === String(over.id)),
      [`${activeId}:name`],
    );
  }

  function confirmDelete(): void {
    const target = pendingDelete;
    if (target === null) {
      return;
    }
    const index = items.findIndex((rule) => rule.id === target.id);
    const neighbour = items[index + 1] ?? items[index - 1];
    remove.mutate(target.id, {
      onSuccess: () => {
        setPendingDelete(null);
        focusKeys.current = neighbour === undefined ? [NEW_RULE_KEY] : [`${neighbour.id}:name`];
      },
      onError: () => {
        setPendingDelete(null);
        notify(t('wl.rules.error.delete'), 'danger', null);
      },
    });
  }

  const cancelText = (name: string, position: number): string => t('wl.rules.drag.cancel', { name, position, total });

  const announcements: Announcements = {
    onDragStart: ({ active }) => {
      const id = String(active.id);
      return t('wl.rules.drag.start', { name: nameOf(id), position: positionOf(id), total });
    },
    onDragOver: () => undefined,
    onDragEnd: ({ active, over }) => {
      const id = String(active.id);
      return over === null || over.id === active.id ? cancelText(nameOf(id), positionOf(id)) : undefined;
    },
    onDragCancel: ({ active }) => {
      const id = String(active.id);
      return cancelText(nameOf(id), positionOf(id));
    },
  };

  const ids = items.map((rule) => rule.id);
  const deleteBody = pendingDelete === null ? '' : t('wl.rules.delete.body', { name: pendingDelete.name });

  return (
    <div ref={rootRef} className={styles.page}>
      <h2 className={styles.heading}>{t('wl.workspace.section.rules')}</h2>
      <p className={styles.hint}>{t('wl.rules.list.orderHint')}</p>
      <div className={styles.toolbar}>
        {readOnly ? null : (
          <>
            <Link
              to="/e/$id/settings/rules/$ruleId"
              params={{ id: endpointId, ruleId: 'new' }}
              search={{ from: 'blank' }}
              className={styles.add}
              data-focus-key={NEW_RULE_KEY}
            >
              {t('wl.rules.new.blank')}
            </Link>
            <Link
              to="/e/$id/settings/rules/$ruleId"
              params={{ id: endpointId, ruleId: 'new' }}
              search={{ from: 'script' }}
              className={styles.secondary}
            >
              {t('wl.rules.new.script')}
            </Link>
          </>
        )}
        {total === 0 ? null : (
          <button
            type="button"
            className={styles.secondary}
            aria-expanded={testing}
            onClick={() => {
              setTestAll(!testing, pinned);
            }}
          >
            {t('wl.rules.testAll.open')}
          </button>
        )}
      </div>
      {total === 0 ? (
        <section className={styles.empty}>
          <h3 className={styles.emptyTitle}>{t('wl.rules.empty.title')}</h3>
          <p className={styles.hint}>{t('wl.rules.empty.body')}</p>
        </section>
      ) : (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          accessibility={{ announcements }}
          onDragEnd={handleDragEnd}
        >
          <SortableContext items={ids} strategy={verticalListSortingStrategy}>
            <ul className={styles.list} aria-label={t('wl.rules.list.label')}>
              {items.map((rule, index) => (
                <RuleCard
                  key={rule.id}
                  endpointId={endpointId}
                  rule={rule}
                  position={index + 1}
                  total={total}
                  readOnly={readOnly}
                  busy={isBusy(rule.id)}
                  onToggle={toggleRule}
                  onMove={moveRule}
                  onDelete={setPendingDelete}
                />
              ))}
            </ul>
          </SortableContext>
        </DndContext>
      )}
      {testing && endpoint !== undefined ? (
        <div className={styles.testAll}>
          <div className={styles.testAllHead}>
            <h3 className={styles.testAllTitle}>{t('wl.rules.testAll.title')}</h3>
            <button
              type="button"
              className={styles.secondary}
              onClick={() => {
                setTestAll(false, null);
              }}
            >
              {t('wl.rules.testAll.close')}
            </button>
          </div>
          <Suspense
            fallback={
              <p role="status" className={styles.loading}>
                {t('wl.workspace.loading')}
              </p>
            }
          >
            <RuleTester
              endpointId={endpointId}
              endpointUrl={endpoint.url}
              draft={null}
              defaultScope="all"
              showScope={false}
              canEdit={!readOnly}
              pick={pick}
              onPickChange={(next) => {
                setTestAll(true, next.mode === 'pinned' ? next.captureId : null);
              }}
            />
          </Suspense>
        </div>
      ) : null}
      <p role="status" className={styles.announcer}>
        {announcement}
      </p>
      {notice === null ? null : (
        <RulesToast
          key={notice.id}
          message={notice.message}
          tone={notice.tone}
          onUndo={undo === null ? null : () => undoToggle(undo)}
          onDismiss={dismissNotice}
        />
      )}
      <ConfirmDialog
        open={pendingDelete !== null}
        title={t('wl.rules.delete.title')}
        body={deleteBody}
        confirmLabel={t('wl.rules.delete.confirm')}
        cancelLabel={t('wl.rules.delete.cancel')}
        busy={remove.isPending}
        onConfirm={confirmDelete}
        onOpenChange={(open) => {
          if (!open) {
            setPendingDelete(null);
          }
        }}
      />
    </div>
  );
}
