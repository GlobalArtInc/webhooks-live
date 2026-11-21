import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from '@tanstack/react-router';
import { useEffect } from 'react';
import { shouldRetryRequest } from '../api/queries';
import { I18nProvider, useLanguage } from '../i18n/provider';
import type { AppRouter } from './router';

export interface AppProvidersProps {
  queryClient: QueryClient;
  router: AppRouter;
}

export function createAppQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: shouldRetryRequest },
    },
  });
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
        <RouterProvider router={router} />
      </I18nProvider>
    </QueryClientProvider>
  );
}
