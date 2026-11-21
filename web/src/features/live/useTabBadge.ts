import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { restoreFavicon, showFaviconBadge, tabTitle } from './tabBadge';

export function useTabBadge(count: number): void {
  const { t } = useTranslation();

  useEffect(() => {
    const base = document.title;
    document.title = tabTitle(base, count, (unread, title) => t('wl.live.title.unread', { count: unread, title }));
    if (count > 0) {
      showFaviconBadge(count);
    } else {
      restoreFavicon();
    }
    return () => {
      document.title = base;
    };
  }, [count, t]);

  useEffect(() => restoreFavicon, []);
}
