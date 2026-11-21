import type { Ref } from 'react';
import type { CapturePick } from '../../../components/CapturePicker/CapturePicker';
import type { ErrorPosition, TesterDraft, TesterScope, TesterTransport, TraceSelection } from './types';

export interface TesterHandle {
  run: () => void;
  selectStep: (stepIndex: number | null) => void;
}

export interface RuleTesterProps {
  endpointId: string;
  endpointUrl: string;
  draft: TesterDraft | null;
  transport?: TesterTransport;
  defaultScope?: TesterScope;
  showScope?: boolean;
  blocked?: boolean;
  dirty?: boolean;
  canEdit?: boolean;
  pick?: CapturePick;
  onPickChange?: (pick: CapturePick) => void;
  stepTitles?: Readonly<Record<string, string>>;
  onSelectionChange?: (selection: TraceSelection | null) => void;
  onGoToError?: (selection: TraceSelection, position: ErrorPosition) => void;
  onInsertVariable?: (path: string) => void;
  ref?: Ref<TesterHandle>;
}
