import * as Dialog from '@radix-ui/react-dialog';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ModalContent } from './ModalContent';

let appRoot: HTMLElement;

beforeEach(() => {
  appRoot = document.createElement('div');
  appRoot.id = 'root';
  document.body.append(appRoot);
});

afterEach(() => {
  cleanup();
  appRoot.remove();
});

interface DemoProps {
  first: boolean;
  second?: boolean;
}

function Demo({ first, second = false }: DemoProps) {
  return (
    <>
      <Dialog.Root open={first}>
        <Dialog.Portal>
          <ModalContent aria-describedby={undefined}>
            <Dialog.Title>First</Dialog.Title>
          </ModalContent>
        </Dialog.Portal>
      </Dialog.Root>
      <Dialog.Root open={second}>
        <Dialog.Portal>
          <ModalContent aria-describedby={undefined}>
            <Dialog.Title>Second</Dialog.Title>
          </ModalContent>
        </Dialog.Portal>
      </Dialog.Root>
    </>
  );
}

describe('ModalContent', () => {
  it('marks the dialog modal and makes the app root inert while it is open', async () => {
    const view = render(<Demo first />, { container: appRoot.appendChild(document.createElement('div')) });

    const dialog = await screen.findByRole('dialog', { name: 'First' });
    expect(dialog.getAttribute('aria-modal')).toBe('true');
    expect(appRoot.hasAttribute('inert')).toBe(true);

    view.rerender(<Demo first={false} />);
    await waitFor(() => expect(appRoot.hasAttribute('inert')).toBe(false));
  });

  it('keeps the root inert until the last open dialog has closed', async () => {
    const view = render(<Demo first second />, { container: appRoot.appendChild(document.createElement('div')) });
    await screen.findByText('Second');

    view.rerender(<Demo first second={false} />);
    await waitFor(() => expect(screen.queryByText('Second')).toBeNull());
    expect(appRoot.hasAttribute('inert')).toBe(true);

    view.rerender(<Demo first={false} second={false} />);
    await waitFor(() => expect(appRoot.hasAttribute('inert')).toBe(false));
  });
});
