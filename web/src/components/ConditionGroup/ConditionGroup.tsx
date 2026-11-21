import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { DEFAULT_LIMITS } from '../../features/rules/conditions/constants';
import {
  addChild,
  countLeaves,
  fitsLimits,
  newGroup,
  newLeaf,
  numberNodes,
  removeNode,
  updateGroup,
  updateLeaf,
  type GroupPatch,
  type LeafPatch,
} from '../../features/rules/conditions/model';
import type {
  ConditionCatalogue,
  ConditionLimits,
  ConditionMode,
  ConditionNumbering,
  GroupDraft,
  IssueMap,
  RowIssue,
  TriggerKind,
} from '../../features/rules/conditions/types';
import type { VariableEntry } from '../VariablePicker/variables';
import styles from './ConditionGroup.module.css';
import { ConditionRow } from './ConditionRow';
import { issueText } from './labels';

export interface ConditionGroupProps {
  root: GroupDraft;
  onChange: (root: GroupDraft) => void;
  catalogue: ConditionCatalogue;
  triggers: readonly TriggerKind[];
  variables: readonly VariableEntry[];
  issues: IssueMap;
  showIssues: boolean;
  readOnly?: boolean;
  limits?: ConditionLimits;
}

interface GroupEnv {
  root: GroupDraft;
  catalogue: ConditionCatalogue;
  triggers: readonly TriggerKind[];
  variables: readonly VariableEntry[];
  issues: IssueMap;
  showIssues: boolean;
  readOnly: boolean;
  limits: ConditionLimits;
  numbering: ConditionNumbering;
  addLeaf: (groupId: string) => void;
  addGroup: (groupId: string) => void;
  remove: (id: string) => void;
  patchLeaf: (id: string, patch: LeafPatch) => void;
  patchGroup: (id: string, patch: GroupPatch) => void;
}

interface GroupNodeProps {
  group: GroupDraft;
  env: GroupEnv;
}

type PendingFocus = { kind: 'subject'; id: string } | { kind: 'add'; groupId: string };

const PROBE_GROUP: GroupDraft = { kind: 'group', id: 'probe-group', negate: false, mode: 'all', children: [] };

const NO_ISSUES: readonly RowIssue[] = [];

function isMode(value: string): value is ConditionMode {
  return value === 'all' || value === 'any';
}

function GroupNode({ group, env }: GroupNodeProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const isRoot = group.id === env.root.id;
  const number = env.numbering.groups.get(group.id) ?? 0;
  const own = env.issues.get(group.id) ?? NO_ISSUES;
  const leafLimit = countLeaves(env.root) >= env.limits.maxLeaves;
  const canAddLeaf = !leafLimit;
  const canAddGroup = fitsLimits(addChild(env.root, group.id, PROBE_GROUP), env.limits);
  const canNegate = fitsLimits(updateGroup(env.root, group.id, { negate: true }), env.limits);
  const hint = leafLimit
    ? t('wl.conditions.limit.leaves', { max: env.limits.maxLeaves })
    : canAddGroup
      ? null
      : t('wl.conditions.limit.depth', { max: env.limits.maxDepth });

  return (
    <div className={styles.group} data-nested={!isRoot} data-group-id={group.id}>
      <div className={styles.header}>
        <label className={styles.modeLabel} htmlFor={`${baseId}-mode`}>
          {t('wl.conditions.mode.label')}
        </label>
        <select
          id={`${baseId}-mode`}
          className={styles.mode}
          disabled={env.readOnly}
          value={group.mode}
          onChange={(event) => {
            if (isMode(event.target.value)) {
              env.patchGroup(group.id, { mode: event.target.value });
            }
          }}
        >
          <option value="all">{t('wl.conditions.mode.all')}</option>
          <option value="any">{t('wl.conditions.mode.any')}</option>
        </select>
        {isRoot || env.readOnly ? null : (
          <>
            <label className={styles.negate}>
              <input
                type="checkbox"
                checked={group.negate}
                disabled={!group.negate && !canNegate}
                aria-label={t('wl.conditions.negateGroup', { number })}
                onChange={(event) => {
                  env.patchGroup(group.id, { negate: event.target.checked });
                }}
              />
              <span>{t('wl.conditions.negate')}</span>
            </label>
            <button
              type="button"
              className={styles.remove}
              aria-label={t('wl.conditions.removeGroup', { number })}
              onClick={() => {
                env.remove(group.id);
              }}
            >
              <span aria-hidden="true">×</span>
            </button>
          </>
        )}
      </div>
      {group.children.length === 0 ? (
        <p className={styles.empty}>{isRoot ? t('wl.conditions.group.empty') : t('wl.conditions.group.emptyNested')}</p>
      ) : (
        <ul className={styles.children}>
          {group.children.map((child) => (
            <li key={child.id} className={styles.child}>
              {child.kind === 'leaf' ? (
                <ConditionRow
                  leaf={child}
                  number={env.numbering.leaves.get(child.id) ?? 0}
                  catalogue={env.catalogue}
                  triggers={env.triggers}
                  variables={env.variables}
                  issues={env.issues.get(child.id) ?? NO_ISSUES}
                  showIssues={env.showIssues}
                  readOnly={env.readOnly}
                  canNegate={fitsLimits(updateLeaf(env.root, child.id, { negate: true }), env.limits)}
                  onChange={(patch) => {
                    env.patchLeaf(child.id, patch);
                  }}
                  onRemove={() => {
                    env.remove(child.id);
                  }}
                />
              ) : (
                <fieldset className={styles.nested}>
                  <legend className={styles.legend}>
                    {t('wl.conditions.group.title', { number: env.numbering.groups.get(child.id) ?? 0 })}
                  </legend>
                  <GroupNode group={child} env={env} />
                </fieldset>
              )}
            </li>
          ))}
        </ul>
      )}
      {own.length === 0 ? null : (
        <ul className={styles.errors}>
          {own.map((issue, index) => (
            <li key={`${issue.code}-${index}`} className={styles.error}>
              {issueText(t, issue)}
            </li>
          ))}
        </ul>
      )}
      {env.readOnly ? null : (
        <div className={styles.footer}>
          <button
            type="button"
            className={styles.add}
            data-add-for={group.id}
            disabled={!canAddLeaf}
            onClick={() => {
              env.addLeaf(group.id);
            }}
          >
            {t('wl.conditions.add')}
          </button>
          <button
            type="button"
            className={styles.add}
            disabled={!canAddGroup || !canAddLeaf}
            onClick={() => {
              env.addGroup(group.id);
            }}
          >
            {t('wl.conditions.addGroup')}
          </button>
          {isRoot ? (
            <span className={styles.count}>
              {t('wl.conditions.count', { used: countLeaves(env.root), max: env.limits.maxLeaves })}
            </span>
          ) : null}
          {hint === null ? null : <span className={styles.limit}>{hint}</span>}
        </div>
      )}
    </div>
  );
}

export function ConditionGroup({
  root,
  onChange,
  catalogue,
  triggers,
  variables,
  issues,
  showIssues,
  readOnly = false,
  limits = DEFAULT_LIMITS,
}: ConditionGroupProps) {
  const { t } = useTranslation();
  const hostRef = useRef<HTMLDivElement>(null);
  const pendingFocus = useRef<PendingFocus | null>(null);
  const [announcement, setAnnouncement] = useState('');
  const numbering = useMemo(() => numberNodes(root), [root]);

  useEffect(() => {
    const pending = pendingFocus.current;
    const host = hostRef.current;
    if (pending === null || host === null) {
      return;
    }
    pendingFocus.current = null;
    const selector =
      pending.kind === 'subject'
        ? `[data-row-id="${pending.id}"] [data-part="subject"]`
        : `[data-add-for="${pending.groupId}"]`;
    host.querySelector<HTMLElement>(selector)?.focus();
  });

  function commit(next: GroupDraft): boolean {
    if (next === root || !fitsLimits(next, limits)) {
      return false;
    }
    onChange(next);
    return true;
  }

  const env: GroupEnv = {
    root,
    catalogue,
    triggers,
    variables,
    issues,
    showIssues,
    readOnly,
    limits,
    numbering,
    addLeaf: (groupId) => {
      const leaf = newLeaf();
      const next = addChild(root, groupId, leaf);
      if (commit(next)) {
        pendingFocus.current = { kind: 'subject', id: leaf.id };
        setAnnouncement(t('wl.conditions.announce.added', { number: numberNodes(next).leaves.get(leaf.id) ?? 0 }));
      }
    },
    addGroup: (groupId) => {
      const group = newGroup();
      const next = addChild(root, groupId, group);
      if (commit(next)) {
        pendingFocus.current = { kind: 'add', groupId: group.id };
        setAnnouncement(
          t('wl.conditions.announce.groupAdded', { number: numberNodes(next).groups.get(group.id) ?? 0 }),
        );
      }
    },
    remove: (id) => {
      const leafNumber = numbering.leaves.get(id);
      const groupNumber = numbering.groups.get(id);
      const parent = findParentId(root, id);
      if (commit(removeNode(root, id))) {
        pendingFocus.current = { kind: 'add', groupId: parent };
        setAnnouncement(
          leafNumber === undefined
            ? t('wl.conditions.announce.groupRemoved', { number: groupNumber ?? 0 })
            : t('wl.conditions.announce.removed', { number: leafNumber }),
        );
      }
    },
    patchLeaf: (id, patch) => {
      commit(updateLeaf(root, id, patch));
    },
    patchGroup: (id, patch) => {
      commit(updateGroup(root, id, patch));
    },
  };

  return (
    <div ref={hostRef} className={styles.host}>
      <GroupNode group={root} env={env} />
      <div role="status" className={styles.visuallyHidden}>
        {announcement}
      </div>
    </div>
  );
}

function findParentId(root: GroupDraft, id: string): string {
  for (const child of root.children) {
    if (child.id === id) {
      return root.id;
    }
    if (child.kind === 'group') {
      const nested = findParentId(child, id);
      if (nested !== '') {
        return nested;
      }
    }
  }
  return '';
}
