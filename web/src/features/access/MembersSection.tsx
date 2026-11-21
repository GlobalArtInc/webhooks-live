import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ApiError } from '../../api/client';
import type { Member, MemberRole, User } from '../../api/types';
import { ConfirmDialog } from '../../components/ConfirmDialog/ConfirmDialog';
import { InviteDialog } from './InviteDialog';
import { usePutMember, useMembers, useRemoveMember } from './useMembers';
import styles from './access.module.css';

export interface MembersSectionProps {
  endpointId: string;
  owner: User;
  canManage: boolean;
  myUserId: string | null;
}

interface RoleChange {
  member: Member;
  role: MemberRole;
}

function personLabel(user: User): string {
  return user.name ?? user.email;
}

function parseRole(value: string): MemberRole {
  return value === 'editor' ? 'editor' : 'viewer';
}

export function MembersSection({ endpointId, owner, canManage, myUserId }: MembersSectionProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const members = useMembers(endpointId, canManage);
  const put = usePutMember(endpointId);
  const remove = useRemoveMember(endpointId);
  const [inviting, setInviting] = useState(false);
  const [roleChange, setRoleChange] = useState<RoleChange | null>(null);
  const [removal, setRemoval] = useState<Member | null>(null);
  const [failure, setFailure] = useState<'forbidden' | 'failed' | null>(null);
  const items = members.data?.items ?? [];

  function reportFailure(error: unknown): void {
    setFailure(error instanceof ApiError && error.status === 403 ? 'forbidden' : 'failed');
  }

  function confirmRoleChange(): void {
    if (roleChange === null) {
      return;
    }
    const { member, role } = roleChange;
    setFailure(null);
    put.mutate(
      { email: member.user.email, role },
      {
        onSettled: () => {
          setRoleChange(null);
        },
        onError: reportFailure,
      },
    );
  }

  function confirmRemoval(): void {
    if (removal === null) {
      return;
    }
    setFailure(null);
    remove.mutate(removal.user.id, {
      onSettled: () => {
        setRemoval(null);
      },
      onError: reportFailure,
    });
  }

  return (
    <section className={styles.block} aria-labelledby={titleId}>
      <div className={styles.sectionHeader}>
        <h3 id={titleId} className={styles.blockTitle}>
          {t('wl.access.members.title')}
        </h3>
        {canManage ? (
          <button
            type="button"
            className={styles.button}
            onClick={() => {
              setInviting(true);
            }}
          >
            {t('wl.access.members.invite.open')}
          </button>
        ) : null}
      </div>
      <p className={styles.hint}>{t('wl.access.members.roles')}</p>
      {canManage ? null : <p className={styles.hint}>{t('wl.access.members.ownerOnly')}</p>}
      {members.isError ? (
        <p role="alert" className={styles.error}>
          {t('wl.access.members.error.load')}
        </p>
      ) : null}
      <ul className={styles.members} aria-label={t('wl.access.members.title')}>
        <li className={styles.member}>
          <span className={styles.person}>
            <span className={styles.personName}>
              {owner.id === myUserId ? t('wl.access.members.you', { name: personLabel(owner) }) : personLabel(owner)}
            </span>
            <span className={styles.personEmail}>{owner.email}</span>
          </span>
          <span className={styles.status}>{t('wl.access.members.role.owner')}</span>
        </li>
        {items.map((member) => (
          <li key={member.user.id} className={styles.member}>
            <span className={styles.person}>
              <span className={styles.personName}>{personLabel(member.user)}</span>
              <span className={styles.personEmail}>{member.user.email}</span>
            </span>
            <span className={styles.actions}>
              <select
                className={styles.select}
                value={member.role}
                aria-label={t('wl.access.members.roleOf', { name: personLabel(member.user) })}
                onChange={(event) => {
                  setRoleChange({ member, role: parseRole(event.target.value) });
                }}
              >
                <option value="viewer">{t('wl.access.members.role.viewer')}</option>
                <option value="editor">{t('wl.access.members.role.editor')}</option>
              </select>
              <button
                type="button"
                className={styles.danger}
                aria-label={t('wl.access.members.removeOf', { name: personLabel(member.user) })}
                onClick={() => {
                  setRemoval(member);
                }}
              >
                {t('wl.access.members.remove')}
              </button>
            </span>
          </li>
        ))}
      </ul>
      {canManage && members.isSuccess && items.length === 0 ? (
        <p className={styles.hint}>{t('wl.access.members.empty')}</p>
      ) : null}
      {failure === null ? null : (
        <p role="alert" className={styles.error}>
          {failure === 'forbidden' ? t('wl.access.members.error.forbidden') : t('wl.access.members.error.failed')}
        </p>
      )}
      <InviteDialog endpointId={endpointId} open={inviting} onOpenChange={setInviting} />
      <ConfirmDialog
        open={roleChange !== null}
        title={t('wl.access.members.changeRole.title')}
        body={
          roleChange === null
            ? ''
            : t('wl.access.members.changeRole.body', {
                name: personLabel(roleChange.member.user),
                role:
                  roleChange.role === 'editor'
                    ? t('wl.access.members.role.editor')
                    : t('wl.access.members.role.viewer'),
              })
        }
        confirmLabel={t('wl.access.members.changeRole.confirm')}
        cancelLabel={t('wl.access.members.cancel')}
        busy={put.isPending}
        onConfirm={confirmRoleChange}
        onOpenChange={(open) => {
          if (!open) {
            setRoleChange(null);
          }
        }}
      />
      <ConfirmDialog
        open={removal !== null}
        title={t('wl.access.members.removeConfirm.title')}
        body={removal === null ? '' : t('wl.access.members.removeConfirm.body', { name: personLabel(removal.user) })}
        confirmLabel={t('wl.access.members.removeConfirm.confirm')}
        cancelLabel={t('wl.access.members.cancel')}
        busy={remove.isPending}
        onConfirm={confirmRemoval}
        onOpenChange={(open) => {
          if (!open) {
            setRemoval(null);
          }
        }}
      />
    </section>
  );
}
