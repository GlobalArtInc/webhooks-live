const SIGN_OUT_PATH = '/auth/logout';

export async function signOut(): Promise<void> {
  try {
    const response = await fetch(SIGN_OUT_PATH, { method: 'POST' });
    if (response.ok) {
      window.location.assign('/');
    }
  } catch {
    return;
  }
}
