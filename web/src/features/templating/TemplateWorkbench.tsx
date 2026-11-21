import { useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { ViewportLayout } from '../endpoint/useViewport';
import { usePanelPrefs, PANEL_MAX_PERCENT, PANEL_MIN_PERCENT, PANEL_STEP_PERCENT } from './panelPrefs';
import type { ProblemItem } from './diagnostics';
import { ProblemList } from './ProblemList';
import { TemplatePreview } from './TemplatePreview';
import styles from './TemplateWorkbench.module.css';
import type { TemplatingModel } from './useTemplating';

export interface TemplateWorkbenchProps {
  model: TemplatingModel;
  layout: ViewportLayout;
  form: ReactNode;
}

type Segment = 'edit' | 'preview' | 'errors';

const SEGMENTS: readonly Segment[] = ['edit', 'preview', 'errors'];

function PreviewHeading() {
  const { t } = useTranslation();
  return <h3 className={styles.heading}>{t('wl.templating.preview.title')}</h3>;
}

function WideBench({ model, form }: Omit<TemplateWorkbenchProps, 'layout'>) {
  return (
    <div className={styles.wide}>
      <div className={styles.column}>
        {form}
        <ProblemList problems={model.problems} onGo={model.goTo} />
      </div>
      <aside className={styles.side}>
        <PreviewHeading />
        <TemplatePreview model={model} />
      </aside>
    </div>
  );
}

function MediumBench({ model, form }: Omit<TemplateWorkbenchProps, 'layout'>) {
  const { t } = useTranslation();
  const [prefs, update] = usePanelPrefs();
  const rangeId = useId();
  const panelId = useId();
  return (
    <div className={styles.medium}>
      <div className={styles.column}>
        {form}
        <ProblemList problems={model.problems} onGo={model.goTo} />
      </div>
      <section className={styles.panel} aria-labelledby={`${panelId}-title`}>
        <div className={styles.panelBar}>
          <h3 id={`${panelId}-title`} className={styles.heading}>
            {t('wl.templating.preview.title')}
          </h3>
          {prefs.hidden ? null : (
            <>
              <label htmlFor={rangeId} className={styles.rangeLabel}>
                {t('wl.templating.preview.height')}
              </label>
              <input
                id={rangeId}
                type="range"
                min={PANEL_MIN_PERCENT}
                max={PANEL_MAX_PERCENT}
                step={PANEL_STEP_PERCENT}
                value={prefs.heightPercent}
                className={styles.range}
                onChange={(event) => {
                  update({ heightPercent: Number(event.target.value) });
                }}
              />
            </>
          )}
          <button
            type="button"
            className={styles.toggle}
            aria-expanded={!prefs.hidden}
            aria-controls={`${panelId}-body`}
            onClick={() => {
              update({ hidden: !prefs.hidden });
            }}
          >
            {prefs.hidden ? t('wl.templating.preview.show') : t('wl.templating.preview.hide')}
          </button>
        </div>
        <div
          id={`${panelId}-body`}
          className={styles.panelBody}
          hidden={prefs.hidden}
          style={{ maxHeight: `${prefs.heightPercent}vh` }}
        >
          <TemplatePreview model={model} />
        </div>
      </section>
    </div>
  );
}

function NarrowBench({ model, form }: Omit<TemplateWorkbenchProps, 'layout'>) {
  const { t } = useTranslation();
  const baseId = useId();
  const [segment, setSegment] = useState<Segment>('edit');
  const tabRefs = useRef(new Map<Segment, HTMLButtonElement>());
  const pendingGo = useRef<ProblemItem | null>(null);
  const { goTo } = model;

  useEffect(() => {
    const item = pendingGo.current;
    if (item !== null && segment === 'edit') {
      pendingGo.current = null;
      goTo(item);
    }
  }, [segment, goTo]);

  function goToProblem(item: ProblemItem): void {
    pendingGo.current = item;
    setSegment('edit');
  }

  function labelOf(item: Segment): string {
    switch (item) {
      case 'edit':
        return t('wl.templating.layout.edit');
      case 'preview':
        return t('wl.templating.layout.preview');
      case 'errors':
        return t('wl.templating.layout.errors', { number: model.errorCount });
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number): void {
    if (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft') {
      return;
    }
    event.preventDefault();
    const step = event.key === 'ArrowRight' ? 1 : SEGMENTS.length - 1;
    const next = SEGMENTS[(index + step) % SEGMENTS.length];
    if (next !== undefined) {
      setSegment(next);
      tabRefs.current.get(next)?.focus();
    }
  }

  return (
    <div className={styles.narrow}>
      <div role="tablist" aria-label={t('wl.templating.layout.segments')} className={styles.tabs}>
        {SEGMENTS.map((item, index) => (
          <button
            key={item}
            type="button"
            role="tab"
            id={`${baseId}-${item}-tab`}
            aria-selected={segment === item}
            aria-controls={`${baseId}-${item}-panel`}
            tabIndex={segment === item ? 0 : -1}
            className={styles.tab}
            ref={(element) => {
              if (element === null) {
                tabRefs.current.delete(item);
              } else {
                tabRefs.current.set(item, element);
              }
            }}
            onClick={() => {
              setSegment(item);
            }}
            onKeyDown={(event) => {
              handleKeyDown(event, index);
            }}
          >
            {labelOf(item)}
          </button>
        ))}
      </div>
      <div
        role="tabpanel"
        id={`${baseId}-edit-panel`}
        aria-labelledby={`${baseId}-edit-tab`}
        hidden={segment !== 'edit'}
        className={styles.column}
      >
        {form}
      </div>
      <div
        role="tabpanel"
        id={`${baseId}-preview-panel`}
        aria-labelledby={`${baseId}-preview-tab`}
        hidden={segment !== 'preview'}
      >
        <TemplatePreview model={model} />
      </div>
      <div
        role="tabpanel"
        id={`${baseId}-errors-panel`}
        aria-labelledby={`${baseId}-errors-tab`}
        hidden={segment !== 'errors'}
      >
        {model.problems.length === 0 ? (
          <p className={styles.note}>{t('wl.templating.problems.none')}</p>
        ) : (
          <ProblemList problems={model.problems} onGo={goToProblem} />
        )}
      </div>
    </div>
  );
}

export function TemplateWorkbench({ model, layout, form }: TemplateWorkbenchProps) {
  if (layout === 'wide') {
    return <WideBench model={model} form={form} />;
  }
  if (layout === 'medium') {
    return <MediumBench model={model} form={form} />;
  }
  return <NarrowBench model={model} form={form} />;
}
