import type { User } from '../../api/types';

export function nameOf(user: Pick<User, 'name'>): string | null {
  const name = user.name?.trim() ?? '';
  return name === '' ? null : name;
}

export function displayNameOf(user: Pick<User, 'name' | 'email'>): string {
  return nameOf(user) ?? user.email;
}

export function initialsOf(user: Pick<User, 'name' | 'email'>): string {
  const words = (nameOf(user) ?? '').split(/\s+/).filter((word) => word !== '');
  const letters =
    words.length > 0
      ? words.slice(0, 2).map((word) => Array.from(word)[0] ?? '')
      : [Array.from(user.email.trim())[0] ?? ''];
  return letters.join('').toLocaleUpperCase();
}
