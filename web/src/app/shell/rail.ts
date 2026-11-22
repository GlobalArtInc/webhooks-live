const ENDPOINT_PATH_PATTERN = /^\/e\//;

export function showsEndpointRail(pathname: string): boolean {
  return pathname === '/' || pathname === '/endpoints' || ENDPOINT_PATH_PATTERN.test(pathname);
}

const COLLAPSING_PATH_PATTERN = /^\/e\/[^/]+\/settings\/(response|rules\/[^/]+)\/?$/;

export function collapsesRail(pathname: string): boolean {
  return COLLAPSING_PATH_PATTERN.test(pathname);
}

const FILLING_PATH_PATTERN = /^\/e\/[^/]+(\/c\/[^/]+)?\/?$/;

export function fillsViewport(pathname: string): boolean {
  return FILLING_PATH_PATTERN.test(pathname);
}
