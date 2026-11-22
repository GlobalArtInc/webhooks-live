import * as Dialog from '@radix-ui/react-dialog';
import type { ComponentProps } from 'react';
import { useInertAppRoot } from './useInertAppRoot';

export type ModalContentProps = ComponentProps<typeof Dialog.Content>;

export function ModalContent(props: ModalContentProps) {
  useInertAppRoot();
  return <Dialog.Content {...props} aria-modal="true" />;
}
