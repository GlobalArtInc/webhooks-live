import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createMemoryHistory, RouterProvider } from '@tanstack/react-router';
import { render } from '@testing-library/react';
import { createAppRouter } from '../../app/router';
import { I18nProvider } from '../../i18n/provider';
import type { BundleLoaders } from '../../i18n/bundles';

const NO_BUNDLES: BundleLoaders = {};

export function renderAppAt(path: string) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const router = createAppRouter({ history: createMemoryHistory({ initialEntries: [path] }), queryClient });
  const view = render(
    <QueryClientProvider client={queryClient}>
      <I18nProvider bundles={NO_BUNDLES}>
        <RouterProvider router={router} />
      </I18nProvider>
    </QueryClientProvider>,
  );
  return { ...view, router };
}
