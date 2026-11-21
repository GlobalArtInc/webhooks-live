import type { QueryClient } from '@tanstack/react-query';
import {
  createRoute,
  createRootRouteWithContext,
  createRouter,
  notFound,
  type RouterHistory,
} from '@tanstack/react-router';
import { DevTokensScreen } from '../routes/dev/DevTokensScreen';
import { DiffScreen } from '../routes/diff/DiffScreen';
import { CaptureScreen } from '../routes/capture/CaptureScreen';
import { EndpointScreen } from '../routes/endpoint/EndpointScreen';
import { EndpointsScreen } from '../routes/endpoints/EndpointsScreen';
import { ForwardingScreen } from '../routes/forwarding/ForwardingScreen';
import { HomeScreen } from '../routes/home/HomeScreen';
import { NotFoundScreen } from '../routes/states/NotFoundScreen';
import { SettingsScreen } from '../routes/settings/SettingsScreen';
import { EndpointSettingsScreen } from '../routes/settings/EndpointSettingsScreen';
import { SignInScreen } from '../routes/signin/SignInScreen';
import { validateCaptureSearch } from '../features/filters/captureSearch';
import { isCaptureId, isEndpointId } from './ids';
import { ensureEndpoint } from './endpointQuery';
import { RouteError } from './RouteError';
import { ShellLayout } from './shell/ShellLayout';
import { safeReturnPath } from './signIn';

export interface RouterContext {
  queryClient: QueryClient;
}

export interface AppRouterOptions {
  history: RouterHistory;
  queryClient: QueryClient;
}

export interface SignInSearch {
  next?: string;
}

export type EndpointSearch = Record<string, never>;

function validateSignInSearch(search: Record<string, unknown>): SignInSearch {
  const next = safeReturnPath(search.next);
  return next === null ? {} : { next };
}

function validateEndpointSearch(): EndpointSearch {
  return {};
}

function assertEndpointId(id: string): void {
  if (!isEndpointId(id)) {
    throw notFound();
  }
}

function assertCaptureId(captureId: string): void {
  if (!isCaptureId(captureId)) {
    throw notFound();
  }
}

const rootRoute = createRootRouteWithContext<RouterContext>()({});

const shellRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'shell',
  component: ShellLayout,
});

const homeRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '/',
  component: HomeScreen,
});

const endpointsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '/endpoints',
  component: EndpointsScreen,
});

const signInRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '/signin',
  validateSearch: validateSignInSearch,
  component: SignInScreen,
});

const settingsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '/settings',
  component: SettingsScreen,
});

const endpointRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '/e/$id',
  validateSearch: validateCaptureSearch,
  beforeLoad: ({ params }) => assertEndpointId(params.id),
  loader: ({ context, params }) => ensureEndpoint(context.queryClient, params.id),
  errorComponent: RouteError,
  notFoundComponent: NotFoundScreen,
  component: EndpointScreen,
});

const captureRoute = createRoute({
  getParentRoute: () => endpointRoute,
  path: 'c/$captureId',
  component: CaptureScreen,
});

const endpointSettingsRoute = createRoute({
  getParentRoute: () => endpointRoute,
  path: 'settings',
  component: EndpointSettingsScreen,
});

const forwardingRoute = createRoute({
  getParentRoute: () => endpointRoute,
  path: 'forwarding',
  component: ForwardingScreen,
});

const diffRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '/e/$id/diff/$a/$b',
  validateSearch: validateEndpointSearch,
  beforeLoad: ({ params }) => {
    assertEndpointId(params.id);
    assertCaptureId(params.a);
    assertCaptureId(params.b);
  },
  loader: ({ context, params }) => ensureEndpoint(context.queryClient, params.id),
  errorComponent: RouteError,
  notFoundComponent: NotFoundScreen,
  component: DiffScreen,
});

const notFoundRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '$',
  component: NotFoundScreen,
});

const devRoutes = import.meta.env.DEV
  ? [
      createRoute({
        getParentRoute: () => rootRoute,
        path: '/dev/tokens',
        component: DevTokensScreen,
      }),
    ]
  : [];

const routeTree = rootRoute.addChildren([
  shellRoute.addChildren([
    homeRoute,
    endpointsRoute,
    signInRoute,
    settingsRoute,
    endpointRoute.addChildren([captureRoute, endpointSettingsRoute, forwardingRoute]),
    diffRoute,
    notFoundRoute,
  ]),
  ...devRoutes,
]);

export function createAppRouter({ history, queryClient }: AppRouterOptions) {
  return createRouter({ routeTree, history, context: { queryClient } });
}

export type AppRouter = ReturnType<typeof createAppRouter>;

declare module '@tanstack/react-router' {
  interface Register {
    router: AppRouter;
  }
}
