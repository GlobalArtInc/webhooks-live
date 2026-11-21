const ENDPOINT_PATH_PATTERN = /^\/e\//;

export function showsEndpointRail(pathname: string): boolean {
  return pathname === '/' || pathname === '/endpoints' || ENDPOINT_PATH_PATTERN.test(pathname);
}
