import { useRef } from 'react';
import { useTranslation } from 'react-i18next';
import styles from './SplitHandle.module.css';

export interface SplitHandleProps {
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (value: number) => void;
  onCommit: (value: number) => void;
}

interface DragStart {
  pointerX: number;
  value: number;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(value)));
}

export function SplitHandle({ value, min, max, step, onChange, onCommit }: SplitHandleProps) {
  const { t } = useTranslation();
  const dragRef = useRef<DragStart | null>(null);

  function widthAt(pointerX: number, start: DragStart): number {
    return clamp(start.value + pointerX - start.pointerX, min, max);
  }

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={t('wl.endpoint.list.resize')}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      tabIndex={0}
      className={styles.handle}
      onPointerDown={(event) => {
        dragRef.current = { pointerX: event.clientX, value };
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        const start = dragRef.current;
        if (start !== null) {
          onChange(widthAt(event.clientX, start));
        }
      }}
      onPointerUp={(event) => {
        const start = dragRef.current;
        if (start !== null) {
          dragRef.current = null;
          onCommit(widthAt(event.clientX, start));
        }
      }}
      onKeyDown={(event) => {
        if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
          event.preventDefault();
          const delta = event.key === 'ArrowLeft' ? -step : step;
          onCommit(clamp(value + delta, min, max));
        }
      }}
    />
  );
}
