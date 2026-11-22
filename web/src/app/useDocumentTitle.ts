import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { setBaseTitle } from './pageTitle';
import { pageTitleOf } from './titles';

export function useDocumentTitle(pathname: string): void {
  const { t } = useTranslation();
  const title = pageTitleOf(pathname, t);

  useEffect(() => {
    setBaseTitle(title);
  }, [title]);
}

export function useStandalonePageTitle(page: string): void {
  const { t } = useTranslation();
  const title = t('wl.title.page', { page, site: t('wl.title.site') });

  useEffect(() => {
    setBaseTitle(title);
  }, [title]);
}
