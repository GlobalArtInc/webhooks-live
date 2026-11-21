export type Dictionary = Readonly<Record<string, string>>;

export type BundleLoader = () => Promise<unknown>;

export type BundleLoaders = Readonly<Record<string, BundleLoader>>;

const PULLED_FILES = import.meta.glob<unknown>('./pulled/*.json', { import: 'default' });

function languageOfPath(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1, -'.json'.length);
}

export const PULLED_BUNDLES: BundleLoaders = Object.fromEntries(
  Object.entries(PULLED_FILES).map(([path, loader]) => [languageOfPath(path), loader]),
);
