import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createI18nInstance } from '../../i18n/setup';
import { SectionNav, type SectionNavItem } from './SectionNav';

const SECTIONS: SectionNavItem[] = [
  { id: 'summary', label: 'Summary' },
  { id: 'body', label: 'Body' },
];

function renderNav(activeId: string | null, onSelect: (id: string) => void) {
  return render(
    <I18nextProvider i18n={createI18nInstance()}>
      <SectionNav sections={SECTIONS} activeId={activeId} onSelect={onSelect} />
    </I18nextProvider>,
  );
}

afterEach(() => {
  cleanup();
});

describe('SectionNav', () => {
  it('marks the active section and reports the section that was clicked', () => {
    const onSelect = vi.fn();
    renderNav('body', onSelect);

    expect(screen.getByRole('button', { name: 'Body' }).getAttribute('aria-current')).toBe('location');
    expect(screen.getByRole('button', { name: 'Summary' }).getAttribute('aria-current')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Summary' }));

    expect(onSelect).toHaveBeenCalledWith('summary');
  });
});
