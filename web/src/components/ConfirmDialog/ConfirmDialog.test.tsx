import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { useRef, useState } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { ConfirmDialog } from './ConfirmDialog';

afterEach(() => {
  cleanup();
});

function Harness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => {
          setOpen(true);
        }}
      >
        Delete all
      </button>
      <ConfirmDialog
        open={open}
        title="Delete all captures?"
        body="Gone for good."
        confirmLabel="Confirm"
        cancelLabel="Cancel"
        busy={false}
        onConfirm={() => {
          setOpen(false);
        }}
        onOpenChange={setOpen}
      />
    </>
  );
}

describe('ConfirmDialog', () => {
  it('returns focus to the button that opened it when Escape closes it', async () => {
    render(<Harness />);
    const trigger = screen.getByRole('button', { name: 'Delete all' });
    trigger.focus();
    fireEvent.click(trigger);
    await screen.findByRole('dialog', { name: 'Delete all captures?' });

    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('returns focus to the opener after Cancel', async () => {
    render(<Harness />);
    const trigger = screen.getByRole('button', { name: 'Delete all' });
    trigger.focus();
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('falls back to the given element when the opener is gone', async () => {
    function MenuHarness() {
      const [open, setOpen] = useState(false);
      const [menuOpen, setMenuOpen] = useState(true);
      const triggerRef = useRef<HTMLButtonElement>(null);
      return (
        <>
          <button ref={triggerRef} type="button">
            Actions
          </button>
          {menuOpen ? (
            <button
              type="button"
              onClick={() => {
                setMenuOpen(false);
                setOpen(true);
              }}
            >
              Item
            </button>
          ) : null}
          <ConfirmDialog
            open={open}
            title="Delete all captures?"
            body="Gone for good."
            confirmLabel="Confirm"
            cancelLabel="Cancel"
            busy={false}
            returnFocusRef={triggerRef}
            onConfirm={() => {
              setOpen(false);
            }}
            onOpenChange={setOpen}
          />
        </>
      );
    }

    render(<MenuHarness />);
    const item = screen.getByRole('button', { name: 'Item' });
    item.focus();
    fireEvent.click(item);
    fireEvent.click(await screen.findByRole('button', { name: 'Cancel' }));

    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Actions' })));
  });
});
