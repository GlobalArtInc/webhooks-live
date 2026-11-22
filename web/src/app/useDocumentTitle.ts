import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { setBaseTitle } from './pageTitle';
import { useCachedEndpointTitle } from '../features/endpoints/titleCache';
import { endpointIdOfPath, pageTitleOf } from './titles';

export function useDocumentTitle(pathname: string): void {
  const { t } = useTranslation();
  const endpointTitle = useCachedEndpointTitle(endpointIdOfPath(pathname));
  const title = pageTitleOf(pathname, t, endpointTitle);

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
