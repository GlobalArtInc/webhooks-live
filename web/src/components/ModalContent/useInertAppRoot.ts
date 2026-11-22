import { useEffect } from 'react';

const APP_ROOT_ID = 'root';

let holders = 0;

export function useInertAppRoot(): void {
  useEffect(() => {
    const root = document.getElementById(APP_ROOT_ID);
    if (root === null) {
      return undefined;
    }
    holders += 1;
    root.setAttribute('inert', '');
    return () => {
      holders -= 1;
      if (holders === 0) {
        root.removeAttribute('inert');
      }
    };
  }, []);
}
