import { useEffect, useState } from 'react';

const TOKEN_PARAM = 'token';

export function tokenFromSearch(search: string): string | null {
  const value = new URLSearchParams(search).get(TOKEN_PARAM);
  return value === null || value === '' ? null : value;
}

export function stripTokenFromUrl(): void {
  const url = new URL(window.location.href);
  if (!url.searchParams.has(TOKEN_PARAM)) {
    return;
  }
  url.searchParams.delete(TOKEN_PARAM);
  window.history.replaceState(window.history.state, '', `${url.pathname}${url.search}${url.hash}`);
}

export function useOneTimeToken(): string | null {
  const [token] = useState(() => tokenFromSearch(window.location.search));
  useEffect(() => {
    stripTokenFromUrl();
  }, []);
  return token;
}
