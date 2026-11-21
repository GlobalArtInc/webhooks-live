import { useId, useRef, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import type { RuleEditorTab } from '../ruleEditorSearch';
import styles from './RuleEditor.module.css';

export interface EditorTabsProps {
  tabs: readonly RuleEditorTab[];
  value: RuleEditorTab;
  onChange: (tab: RuleEditorTab) => void;
  panelId: (tab: RuleEditorTab) => string;
  tabId: (tab: RuleEditorTab) => string;
}

export function EditorTabs({ tabs, value, onChange, panelId, tabId }: EditorTabsProps) {
  const { t } = useTranslation();
  const listId = useId();
  const refs = useRef(new Map<RuleEditorTab, HTMLButtonElement>());

  function labelOf(tab: RuleEditorTab): string {
    switch (tab) {
      case 'build':
        return t('wl.ruleeditor.tab.build');
      case 'test':
        return t('wl.ruleeditor.tab.test');
      case 'runs':
        return t('wl.ruleeditor.tab.runs');
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number): void {
    let next = index;
    if (event.key === 'ArrowRight') {
      next = (index + 1) % tabs.length;
    } else if (event.key === 'ArrowLeft') {
      next = (index - 1 + tabs.length) % tabs.length;
    } else if (event.key === 'Home') {
      next = 0;
    } else if (event.key === 'End') {
      next = tabs.length - 1;
    } else {
      return;
    }
    event.preventDefault();
    const target = tabs[next];
    if (target !== undefined) {
      onChange(target);
      refs.current.get(target)?.focus();
    }
  }

  return (
    <div id={listId} role="tablist" aria-label={t('wl.ruleeditor.tab.label')} className={styles.tabs}>
      {tabs.map((tab, index) => (
        <button
          key={tab}
          id={tabId(tab)}
          ref={(element) => {
            if (element === null) {
              refs.current.delete(tab);
            } else {
              refs.current.set(tab, element);
            }
          }}
          type="button"
          role="tab"
          className={styles.tab}
          aria-selected={tab === value}
          aria-controls={panelId(tab)}
          tabIndex={tab === value ? 0 : -1}
          onClick={() => {
            onChange(tab);
          }}
          onKeyDown={(event) => {
            handleKeyDown(event, index);
          }}
        >
          {labelOf(tab)}
        </button>
      ))}
    </div>
  );
}
