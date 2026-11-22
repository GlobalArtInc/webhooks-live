import { useId, useState } from 'react';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import type { SchemaConstraint, SchemaNode } from './schemaTree';
import { TextBlocks } from './TextBlocks';
import styles from './SchemaView.module.css';

export interface SchemaBlockProps {
  node: SchemaNode;
  name: string;
  defaultOpen?: boolean;
}

interface SchemaRowProps {
  node: SchemaNode;
  label: string;
  showRequired: boolean;
}

interface SchemaMetaProps {
  node: SchemaNode;
}

interface SchemaChildrenProps {
  node: SchemaNode;
}

function typeLabel(t: TFunction, node: SchemaNode): string {
  if (node.refName !== null) {
    return node.refName;
  }
  if (node.type === 'array' && node.items !== null) {
    return t('wl.apireference.schema.arrayOf', { item: typeLabel(t, node.items) });
  }
  return node.type ?? t('wl.apireference.schema.any');
}

function constraintText(t: TFunction, constraint: SchemaConstraint): string {
  const value = constraint.value;
  switch (constraint.kind) {
    case 'minimum':
      return t('wl.apireference.constraint.minimum', { value });
    case 'maximum':
      return t('wl.apireference.constraint.maximum', { value });
    case 'exclusiveMinimum':
      return t('wl.apireference.constraint.exclusiveMinimum', { value });
    case 'exclusiveMaximum':
      return t('wl.apireference.constraint.exclusiveMaximum', { value });
    case 'multipleOf':
      return t('wl.apireference.constraint.multipleOf', { value });
    case 'minLength':
      return t('wl.apireference.constraint.minLength', { value });
    case 'maxLength':
      return t('wl.apireference.constraint.maxLength', { value });
    case 'pattern':
      return t('wl.apireference.constraint.pattern', { value });
    case 'minItems':
      return t('wl.apireference.constraint.minItems', { value });
    case 'maxItems':
      return t('wl.apireference.constraint.maxItems', { value });
    case 'uniqueItems':
      return t('wl.apireference.constraint.uniqueItems');
    case 'minProperties':
      return t('wl.apireference.constraint.minProperties', { value });
    case 'maxProperties':
      return t('wl.apireference.constraint.maxProperties', { value });
  }
}

export function SchemaMeta({ node }: SchemaMetaProps) {
  const { t } = useTranslation();
  const facts: string[] = [];
  if (node.enumValues.length > 0) {
    facts.push(t('wl.apireference.schema.enum', { values: node.enumValues.join(', ') }));
  }
  for (const constraint of node.constraints) {
    facts.push(constraintText(t, constraint));
  }
  if (node.defaultValue !== null) {
    facts.push(t('wl.apireference.schema.default', { value: node.defaultValue }));
  }
  if (node.example !== null) {
    facts.push(t('wl.apireference.schema.example', { value: node.example }));
  }
  if (node.readOnly) {
    facts.push(t('wl.apireference.schema.readOnly'));
  }
  if (node.writeOnly) {
    facts.push(t('wl.apireference.schema.writeOnly'));
  }
  if (node.deprecated) {
    facts.push(t('wl.apireference.schema.deprecated'));
  }
  if (node.recursive) {
    facts.push(
      node.refName === null
        ? t('wl.apireference.schema.recursiveAnonymous')
        : t('wl.apireference.schema.recursive', { name: node.refName }),
    );
  }
  if (node.truncated) {
    facts.push(t('wl.apireference.schema.truncated'));
  }
  return (
    <>
      {node.recursive ? null : (
        <span className={styles.type}>
          {typeLabel(t, node)}
          {node.format === null ? null : ` (${node.format})`}
          {node.nullable ? ` ${t('wl.apireference.schema.nullable')}` : null}
        </span>
      )}
      {facts.length === 0 ? null : <span className={styles.facts}>{facts.join('; ')}</span>}
    </>
  );
}

function SchemaChildren({ node }: SchemaChildrenProps) {
  const { t } = useTranslation();
  const properties = node.properties();
  return (
    <ul className={styles.tree}>
      {properties.map((property) => (
        <SchemaRow key={property.name ?? ''} node={property} label={property.name ?? ''} showRequired />
      ))}
      {node.additional === null ? null : (
        <SchemaRow node={node.additional} label={t('wl.apireference.schema.anyKey')} showRequired={false} />
      )}
      {node.items === null || !node.expandable ? null : (
        <SchemaRow node={node.items} label={t('wl.apireference.schema.items')} showRequired={false} />
      )}
      {node.alternatives.map((alternative, index) => (
        <SchemaRow
          key={index}
          node={alternative}
          label={t('wl.apireference.schema.option', { index: index + 1 })}
          showRequired={false}
        />
      ))}
    </ul>
  );
}

function SchemaRow({ node, label, showRequired }: SchemaRowProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const panelId = useId();
  return (
    <li className={styles.row}>
      <div className={styles.line}>
        {node.expandable ? (
          <button
            type="button"
            className={styles.toggle}
            aria-expanded={open}
            aria-controls={panelId}
            onClick={() => setOpen((current) => !current)}
          >
            <span className={styles.chevron} aria-hidden="true" data-open={open} />
            {label}
          </button>
        ) : (
          <span className={styles.name}>{label}</span>
        )}
        <SchemaMeta node={node} />
        {showRequired ? (
          <span className={styles.flag} data-required={node.required}>
            {node.required ? t('wl.apireference.schema.required') : t('wl.apireference.schema.optional')}
          </span>
        ) : null}
      </div>
      {node.description === null ? null : (
        <div className={styles.description}>
          <TextBlocks text={node.description} />
        </div>
      )}
      <div id={panelId} hidden={!open}>
        {open ? <SchemaChildren node={node} /> : null}
      </div>
    </li>
  );
}

export function SchemaBlock({ node, name, defaultOpen = false }: SchemaBlockProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(defaultOpen);
  const panelId = useId();
  return (
    <div className={styles.block}>
      <div className={styles.line}>
        <SchemaMeta node={node} />
        {node.expandable ? (
          <button
            type="button"
            className={styles.expand}
            aria-expanded={open}
            aria-controls={panelId}
            aria-label={
              open
                ? t('wl.apireference.schema.collapseLabel', { name })
                : t('wl.apireference.schema.expandLabel', { name })
            }
            onClick={() => setOpen((current) => !current)}
          >
            {open ? t('wl.apireference.schema.collapse') : t('wl.apireference.schema.expand')}
          </button>
        ) : null}
      </div>
      {node.description === null ? null : (
        <div className={styles.description}>
          <TextBlocks text={node.description} />
        </div>
      )}
      <div id={panelId} hidden={!open}>
        {open ? <SchemaChildren node={node} /> : null}
      </div>
    </div>
  );
}
