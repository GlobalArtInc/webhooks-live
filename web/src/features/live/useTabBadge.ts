import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { setUnreadTitle } from '../../app/pageTitle';
import { restoreFavicon, showFaviconBadge } from './tabBadge';

export function useTabBadge(count: number): void {
  const { t } = useTranslation();

  useEffect(() => {
    setUnreadTitle(count > 0 ? (title) => t('wl.live.title.unread', { count, title }) : null);
    if (count > 0) {
      showFaviconBadge(count);
    } else {
      restoreFavicon();
    }
    return () => {
      setUnreadTitle(null);
    };
  }, [count, t]);

  useEffect(() => restoreFavicon, []);
}
