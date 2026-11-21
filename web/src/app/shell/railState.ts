import { createContext, useContext } from 'react';

export interface RailState {
  collapsed: boolean;
  drawerOpen: boolean;
  toggleCollapsed: () => void;
  toggleDrawer: () => void;
  closeDrawer: () => void;
}

const noop = () => undefined;

export const RailContext = createContext<RailState>({
  collapsed: false,
  drawerOpen: false,
  toggleCollapsed: noop,
  toggleDrawer: noop,
  closeDrawer: noop,
});

export function useRailState(): RailState {
  return useContext(RailContext);
}
