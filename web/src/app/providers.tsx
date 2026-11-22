import { MutationCache, QueryCache, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from '@tanstack/react-router';
import { useEffect } from 'react';
import { refreshUsageOnLimit, usageQueryKeys } from '../api/usage';
import { shouldRetryRequest } from '../api/queries';
import { syncEndpointTitles } from '../features/endpoints/titleSync';
import { ShortcutProvider } from '../features/shortcuts';
import { I18nProvider, useLanguage } from '../i18n/provider';
import type { AppRouter } from './router';

export interface AppProvidersProps {
  queryClient: QueryClient;
  router: AppRouter;
}

const USAGE_KEY_HEAD = usageQueryKeys.usage()[0];

export function createAppQueryClient(): QueryClient {
  const queryClient: QueryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: shouldRetryRequest },
    },
    queryCache: new QueryCache({
      onError: (error, query) => {
        if (query.queryKey[0] !== USAGE_KEY_HEAD) {
          refreshUsageOnLimit(queryClient, error);
        }
      },
    }),
    mutationCache: new MutationCache({
      onError: (error) => refreshUsageOnLimit(queryClient, error),
    }),
  });
  syncEndpointTitles(queryClient);
  return queryClient;
}

function LanguageDocumentSync() {
  const { language } = useLanguage();

  useEffect(() => {
    document.documentElement.lang = language;
  }, [language]);

  return null;
}

export function AppProviders({ queryClient, router }: AppProvidersProps) {
  return (
    <QueryClientProvider client={queryClient}>
      <I18nProvider>
        <LanguageDocumentSync />
        <ShortcutProvider>
          <RouterProvider router={router} />
        </ShortcutProvider>
      </I18nProvider>
    </QueryClientProvider>
  );
}
