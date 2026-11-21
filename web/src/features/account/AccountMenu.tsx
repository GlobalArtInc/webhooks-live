import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { Link } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import type { User } from '../../api/types';
import { signOut } from '../../app/signOut';
import { displayNameOf, initialsOf } from './identity';
import styles from './AccountMenu.module.css';

export interface AccountMenuProps {
  user: User;
}

export function AccountMenu({ user }: AccountMenuProps) {
  const { t } = useTranslation();
  const name = displayNameOf(user);
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <button type="button" className={styles.trigger} aria-label={t('wl.account.menu.trigger', { name })}>
          <span className={styles.avatar} aria-hidden="true" data-initials={initialsOf(user)} />
          <span className={styles.name}>{name}</span>
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className={styles.menu} align="end" sideOffset={4}>
          <DropdownMenu.Item asChild className={styles.menuItem}>
            <Link to="/account">{t('wl.account.menu.account')}</Link>
          </DropdownMenu.Item>
          <DropdownMenu.Item asChild className={styles.menuItem}>
            <Link to="/settings/tokens">{t('wl.account.menu.tokens')}</Link>
          </DropdownMenu.Item>
          <DropdownMenu.Item asChild className={styles.menuItem}>
            <Link to="/usage">{t('wl.account.menu.usage')}</Link>
          </DropdownMenu.Item>
          <DropdownMenu.Item asChild className={styles.menuItem}>
            <Link to="/settings">{t('wl.account.preferences')}</Link>
          </DropdownMenu.Item>
          <DropdownMenu.Separator className={styles.separator} />
          <DropdownMenu.Item className={styles.menuItem} onSelect={() => void signOut()}>
            {t('wl.account.signOut')}
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
