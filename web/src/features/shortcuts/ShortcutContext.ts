import { createContext } from 'react';
import { createShortcutStore, type ShortcutStore } from './store';

export const ShortcutContext = createContext<ShortcutStore>(createShortcutStore());
