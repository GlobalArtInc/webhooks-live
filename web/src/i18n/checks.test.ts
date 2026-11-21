import { afterEach, describe, expect, it } from 'vitest';
import { checkTree } from '../../tools/i18n/checks.mjs';
import { removeTree, writeTree } from '../../tools/i18n/fixtures.mjs';

const roots: string[] = [];

function tree(files: Readonly<Record<string, string>>): string {
  const root = writeTree(files);
  roots.push(root);
  return root;
}

function json(value: Readonly<Record<string, string>>): string {
  return JSON.stringify(value, null, 2);
}

const CLEAN_FILES: Readonly<Record<string, string>> = {
  'src/i18n/en/common.json': json({ 'wl.common.language': 'Language' }),
  'src/i18n/en/sample.json': json({
    'wl.sample.newCaptures_one': '{{count}} new capture',
    'wl.sample.newCaptures_other': '{{count}} new captures',
  }),
  'src/app/Page.tsx': [
    "export const Page = () => <p>{t('wl.common.language')}{t('wl.sample.newCaptures', { count: 2 })}</p>;",
    'export const Label = () => <Trans i18nKey="wl.common.language" />;',
  ].join('\n'),
  'src/i18n/pulled/ru.json': json({
    'wl.common.language': 'Язык',
    'wl.sample.newCaptures_one': '{{count}} перехват',
    'wl.sample.newCaptures_other': '{{count}} перехватов',
  }),
};

afterEach(() => {
  for (const root of roots.splice(0)) {
    removeTree(root);
  }
});

describe('checkTree', () => {
  it('passes a tree whose references, plurals and pulled placeholders agree with the source', () => {
    const report = checkTree(tree(CLEAN_FILES));

    expect(report.errors).toEqual([]);
    expect(report.warnings).toEqual([]);
  });

  it('reports a literal key that the English source does not have', () => {
    const report = checkTree(
      tree({
        ...CLEAN_FILES,
        'src/app/Missing.tsx': "export const Missing = () => <p>{t('wl.common.missing')}</p>;",
      }),
    );

    expect(report.errors).toEqual([
      'src/app/Missing.tsx: wl.common.missing is used but missing from the English source',
    ]);
  });

  it('reports a source key that does not match the wl.<area>.<element> shape', () => {
    const report = checkTree(
      tree({
        ...CLEAN_FILES,
        'src/i18n/en/common.json': json({ 'wl.Common.language': 'Language' }),
      }),
    );

    expect(report.errors.join('\n')).toContain('wl.Common.language does not match wl.<area>.<element>[.<qualifier>]');
  });

  it('reports a plural base that has no _other form', () => {
    const report = checkTree(
      tree({
        ...CLEAN_FILES,
        'src/i18n/en/sample.json': json({ 'wl.sample.newCaptures_one': '{{count}} new capture' }),
        'src/app/Page.tsx': "export const Page = () => <p>{t('wl.common.language')}</p>;",
      }),
    );

    expect(report.errors).toEqual([
      'src/i18n/en/sample.json: wl.sample.newCaptures has no wl.sample.newCaptures_other form',
    ]);
  });

  it('reports an empty source value', () => {
    const report = checkTree(
      tree({
        ...CLEAN_FILES,
        'src/i18n/en/common.json': json({ 'wl.common.language': '' }),
      }),
    );

    expect(report.errors).toEqual(['src/i18n/en/common.json: wl.common.language is empty']);
  });

  it('reports a pulled translation whose placeholder name differs from English', () => {
    const report = checkTree(
      tree({
        ...CLEAN_FILES,
        'src/i18n/pulled/ru.json': json({ 'wl.sample.newCaptures_one': '{{total}} перехват' }),
      }),
    );

    expect(report.errors).toEqual([
      'src/i18n/pulled/ru.json: wl.sample.newCaptures_one has placeholders [total], English has [count]',
    ]);
  });

  it('reports a source key that no code references as a warning, not an error', () => {
    const report = checkTree(
      tree({
        ...CLEAN_FILES,
        'src/i18n/en/common.json': json({ 'wl.common.language': 'Language', 'wl.common.unused': 'Unused' }),
      }),
    );

    expect(report.errors).toEqual([]);
    expect(report.warnings).toEqual(['src/i18n/en/common.json: wl.common.unused is never used in the code']);
  });
});
