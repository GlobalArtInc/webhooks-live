import * as Dialog from '@radix-ui/react-dialog';
import { useId, useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { CLONE_PARTS, useCloneEndpoint, type ClonePart, type CloneResult } from '../../api/general';
import styles from './CloneDialog.module.css';

type PartKey =
  | 'wl.general.clone.part.settings'
  | 'wl.general.clone.part.response'
  | 'wl.general.clone.part.forwarding'
  | 'wl.general.clone.part.rules'
  | 'wl.general.clone.part.schedules'
  | 'wl.general.clone.part.channels'
  | 'wl.general.clone.part.verifier';

const PART_KEYS: Readonly<Record<ClonePart, PartKey>> = {
  settings: 'wl.general.clone.part.settings',
  response: 'wl.general.clone.part.response',
  forwarding: 'wl.general.clone.part.forwarding',
  rules: 'wl.general.clone.part.rules',
  schedules: 'wl.general.clone.part.schedules',
  channels: 'wl.general.clone.part.channels',
  verifier: 'wl.general.clone.part.verifier',
};

export interface CloneDialogProps {
  endpointId: string;
  title: string | null;
  onClose: () => void;
  onCloseAutoFocus: (event: Event) => void;
  onCloned: (clone: CloneResult) => void;
}

export function CloneDialog({ endpointId, title, onClose, onCloseAutoFocus, onCloned }: CloneDialogProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const clone = useCloneEndpoint(endpointId);
  const [name, setName] = useState(() =>
    title === null || title.trim() === ''
      ? t('wl.general.clone.defaultUntitled', { id: endpointId })
      : t('wl.general.clone.defaultTitle', { title }),
  );
  const [selected, setSelected] = useState<ClonePart[]>([...CLONE_PARTS]);
  const [created, setCreated] = useState<CloneResult | null>(null);
  const nameId = `${baseId}-name`;
  const trimmed = name.trim();

  function toggle(part: ClonePart, checked: boolean): void {
    setSelected((current) => (checked ? [...current, part] : current.filter((item) => item !== part)));
  }

  function submit(event: FormEvent<HTMLFormElement>): void {
    event.preventDefault();
    if (clone.isPending || trimmed === '') {
      return;
    }
    clone.mutate(
      { title: trimmed, parts: CLONE_PARTS.filter((part) => selected.includes(part)) },
      {
        onSuccess: (result) => {
          if (result.secrets_missing.length === 0) {
            onCloned(result);
            return;
          }
          setCreated(result);
        },
      },
    );
  }

  return (
    <Dialog.Root
      open
      onOpenChange={(next) => {
        if (!next) {
          onClose();
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className={styles.overlay} />
        <Dialog.Content className={styles.content} onCloseAutoFocus={onCloseAutoFocus}>
          <Dialog.Title className={styles.title}>{t('wl.general.clone.title')}</Dialog.Title>
          <Dialog.Description className={styles.body}>{t('wl.general.clone.body')}</Dialog.Description>
          {created === null ? (
            <form className={styles.form} onSubmit={submit}>
              <div className={styles.field}>
                <label htmlFor={nameId} className={styles.label}>
                  {t('wl.general.clone.name')}
                </label>
                <input
                  id={nameId}
                  type="text"
                  autoComplete="off"
                  className={styles.input}
                  value={name}
                  onChange={(event) => {
                    setName(event.target.value);
                  }}
                />
              </div>
              <fieldset className={styles.parts}>
                <legend className={styles.label}>{t('wl.general.clone.parts.legend')}</legend>
                {CLONE_PARTS.map((part) => (
                  <label key={part} className={styles.choice}>
                    <input
                      type="checkbox"
                      checked={selected.includes(part)}
                      onChange={(event) => {
                        toggle(part, event.target.checked);
                      }}
                    />
                    {t(PART_KEYS[part])}
                  </label>
                ))}
              </fieldset>
              {clone.isPending ? (
                <p role="status" className={styles.body}>
                  {t('wl.general.clone.cloning')}
                </p>
              ) : null}
              {clone.isError ? (
                <p role="alert" className={styles.error}>
                  {t('wl.general.clone.failed')}
                </p>
              ) : null}
              <div className={styles.actions}>
                <Dialog.Close asChild>
                  <button type="button" className={styles.secondary}>
                    {t('wl.general.clone.cancel')}
                  </button>
                </Dialog.Close>
                <button type="submit" className={styles.primary} disabled={clone.isPending || trimmed === ''}>
                  {t('wl.general.clone.submit')}
                </button>
              </div>
            </form>
          ) : (
            <div className={styles.form}>
              <h3 className={styles.subtitle}>{t('wl.general.clone.secrets.title')}</h3>
              <p className={styles.body}>{t('wl.general.clone.secrets.body')}</p>
              <ul className={styles.names}>
                {created.secrets_missing.map((secret) => (
                  <li key={secret}>
                    <code>{secret}</code>
                  </li>
                ))}
              </ul>
              <div className={styles.actions}>
                <button
                  type="button"
                  className={styles.primary}
                  onClick={() => {
                    onCloned(created);
                  }}
                >
                  {t('wl.general.clone.openNew')}
                </button>
              </div>
            </div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
