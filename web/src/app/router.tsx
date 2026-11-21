import type { QueryClient } from '@tanstack/react-query';
import {
  createRoute,
  createRootRouteWithContext,
  createRouter,
  lazyRouteComponent,
  notFound,
  redirect,
  type RouterHistory,
} from '@tanstack/react-router';
import { DevEditorsScreen } from '../routes/dev/DevEditorsScreen';
import { DevScriptScreen } from '../routes/dev/DevScriptScreen';
import { DevStepsScreen } from '../routes/dev/DevStepsScreen';
import { DevTokensScreen } from '../routes/dev/DevTokensScreen';
import { DiffScreen } from '../routes/diff/DiffScreen';
import { CaptureScreen } from '../routes/capture/CaptureScreen';
import { EndpointScreen } from '../routes/endpoint/EndpointScreen';
import { EndpointsScreen } from '../routes/endpoints/EndpointsScreen';
import { HomeScreen } from '../routes/home/HomeScreen';
import { NotFoundScreen } from '../routes/states/NotFoundScreen';
import { SettingsScreen } from '../routes/settings/SettingsScreen';
import { EndpointSettingsScreen } from '../routes/settings/EndpointSettingsScreen';
import { SettingsIndexScreen } from '../routes/settings/SettingsIndexScreen';
import { SettingsSectionScreen } from '../routes/settings/SettingsSectionScreen';
import { SignInScreen } from '../routes/signin/SignInScreen';
import { validateCaptureSearch } from '../features/filters/captureSearch';
import { validateComposeSearch } from '../features/composer/search';
import { validateEndpointsSearch } from '../features/folders/search';
import { isCaptureId, isEndpointId } from './ids';
import { ensureEndpoint } from './endpointQuery';
import { isWorkspaceSectionId } from '../features/workspace/sections';
import { RuleEditorPlaceholder } from '../features/rules/RuleEditorPlaceholder';
import { validateRuleEditorSearch } from '../features/rules/ruleEditorSearch';
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

function assertWorkspaceSection(section: string): void {
  if (!isWorkspaceSectionId(section)) {
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
  validateSearch: validateEndpointsSearch,
  component: EndpointsRouteScreen,
});

export function EndpointsRouteScreen() {
  const search = endpointsRoute.useSearch();
  return <EndpointsScreen search={search} />;
}

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

const composeRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '/send',
  validateSearch: validateComposeSearch,
  component: lazyRouteComponent(() => import('../routes/compose/ComposeScreen'), 'ComposeScreen'),
});

const tokensRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: '/settings/tokens',
  component: lazyRouteComponent(() => import('../routes/tokens/TokensScreen'), 'TokensScreen'),
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

const settingsIndexRoute = createRoute({
  getParentRoute: () => endpointSettingsRoute,
  path: '/',
  component: SettingsIndexScreen,
});

const settingsSectionRoute = createRoute({
  getParentRoute: () => endpointSettingsRoute,
  path: '$section',
  beforeLoad: ({ params }) => assertWorkspaceSection(params.section),
  component: SettingsSectionScreen,
});

const ruleEditorRoute = createRoute({
  getParentRoute: () => endpointSettingsRoute,
  path: 'rules/$ruleId',
  validateSearch: validateRuleEditorSearch,
  component: RuleEditorPlaceholder,
});

const forwardingRedirectRoute = createRoute({
  getParentRoute: () => endpointRoute,
  path: 'forwarding',
  beforeLoad: ({ params }) => {
    throw redirect({ to: '/e/$id/settings/$section', params: { id: params.id, section: 'forwarding' } });
  },
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
      createRoute({
        getParentRoute: () => rootRoute,
        path: '/dev/editors',
        component: DevEditorsScreen,
      }),
      createRoute({
        getParentRoute: () => rootRoute,
        path: '/dev/script',
        component: DevScriptScreen,
      }),
      createRoute({
        getParentRoute: () => rootRoute,
        path: '/dev/steps',
        component: DevStepsScreen,
      }),
    ]
  : [];

const routeTree = rootRoute.addChildren([
  shellRoute.addChildren([
    homeRoute,
    endpointsRoute,
    signInRoute,
    settingsRoute,
    composeRoute,
    tokensRoute,
    endpointRoute.addChildren([
      captureRoute,
      endpointSettingsRoute.addChildren([settingsIndexRoute, settingsSectionRoute, ruleEditorRoute]),
      forwardingRedirectRoute,
    ]),
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
