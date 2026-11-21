import { QueryClient } from '@tanstack/react-query';
import { createMemoryHistory } from '@tanstack/react-router';
import { describe, expect, it } from 'vitest';
import { CaptureScreen } from '../routes/capture/CaptureScreen';
import { DevTokensScreen } from '../routes/dev/DevTokensScreen';
import { DiffScreen } from '../routes/diff/DiffScreen';
import { EndpointScreen } from '../routes/endpoint/EndpointScreen';
import { EndpointsScreen } from '../routes/endpoints/EndpointsScreen';
import { ForwardingScreen } from '../routes/forwarding/ForwardingScreen';
import { HomeScreen } from '../routes/home/HomeScreen';
import { NotFoundScreen } from '../routes/states/NotFoundScreen';
import { SettingsScreen } from '../routes/settings/SettingsScreen';
import { EndpointSettingsScreen } from '../routes/settings/EndpointSettingsScreen';
import { SignInScreen } from '../routes/signin/SignInScreen';
import { createAppRouter } from './router';

const ENDPOINT_ID = '0123456789abcdefghjkmnpq';
const CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a21';
const OTHER_CAPTURE_ID = '0193a1c2-7e4b-7c11-9a2f-3b5d1e0f4a22';

interface RouteCase {
  path: string;
  screen: unknown;
  params: Record<string, string>;
}

const ROUTE_CASES: RouteCase[] = [
  { path: '/', screen: HomeScreen, params: {} },
  { path: '/endpoints', screen: EndpointsScreen, params: {} },
  { path: '/signin', screen: SignInScreen, params: {} },
  { path: '/settings', screen: SettingsScreen, params: {} },
  { path: `/e/${ENDPOINT_ID}`, screen: EndpointScreen, params: { id: ENDPOINT_ID } },
  {
    path: `/e/${ENDPOINT_ID}/c/${CAPTURE_ID}`,
    screen: CaptureScreen,
    params: { id: ENDPOINT_ID, captureId: CAPTURE_ID },
  },
  { path: `/e/${ENDPOINT_ID}/settings`, screen: EndpointSettingsScreen, params: { id: ENDPOINT_ID } },
  { path: `/e/${ENDPOINT_ID}/forwarding`, screen: ForwardingScreen, params: { id: ENDPOINT_ID } },
  {
    path: `/e/${ENDPOINT_ID}/diff/${CAPTURE_ID}/${OTHER_CAPTURE_ID}`,
    screen: DiffScreen,
    params: { id: ENDPOINT_ID, a: CAPTURE_ID, b: OTHER_CAPTURE_ID },
  },
  { path: '/dev/tokens', screen: DevTokensScreen, params: {} },
  { path: '/no/such/page', screen: NotFoundScreen, params: { '*': 'no/such/page', _splat: 'no/such/page' } },
];

describe('route table', () => {
  it.each(ROUTE_CASES)('resolves $path to its screen with typed params', ({ path, screen, params }) => {
    const router = createAppRouter({
      history: createMemoryHistory({ initialEntries: [path] }),
      queryClient: new QueryClient(),
    });
    const leaf = router.matchRoutes(path).at(-1);

    expect(leaf).toBeDefined();
    expect(leaf?.params).toEqual(params);
    expect(leaf === undefined ? undefined : router.routesById[leaf.routeId].options.component).toBe(screen);
  });
});
