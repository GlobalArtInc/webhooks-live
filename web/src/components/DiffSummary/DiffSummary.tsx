import { useId } from 'react';
import { useTranslation } from 'react-i18next';
import { isIdentical } from '../../features/diff/summary';
import type { NormalisedSummary, StructuralSummary } from '../../features/diff/types';
import styles from './DiffSummary.module.css';

export interface DiffSummaryProps {
  left: NormalisedSummary;
  right: NormalisedSummary;
  leftTime: string;
  rightTime: string;
  comparison: StructuralSummary;
}

export function DiffSummary({ left, right, leftTime, rightTime, comparison }: DiffSummaryProps) {
  const { t } = useTranslation();
  const headingId = useId();
  const { headers } = comparison;

  const bodyState =
    comparison.body === 'same'
      ? t('wl.diff.result.same')
      : comparison.body === 'different'
        ? t('wl.diff.result.different')
        : t('wl.diff.result.notCompared');

  const rows = [
    {
      label: t('wl.diff.field.method'),
      leftValue: left.method,
      rightValue: right.method,
      equal: comparison.fields.method,
    },
    {
      label: t('wl.diff.field.path'),
      leftValue: left.path,
      rightValue: right.path,
      equal: comparison.fields.path,
    },
    {
      label: t('wl.diff.field.query'),
      leftValue: left.query,
      rightValue: right.query,
      equal: comparison.fields.query,
    },
  ];

  return (
    <section className={styles.section} aria-labelledby={headingId}>
      <h2 id={headingId} className={styles.heading}>
        {t('wl.diff.summary.heading')}
      </h2>
      <p className={styles.text}>
        {t('wl.diff.summary.headers', {
          changed: headers.changed,
          added: headers.added,
          removed: headers.removed,
          same: headers.same,
          hidden: headers.hidden,
        })}
      </p>
      <p className={styles.text}>{t('wl.diff.summary.body', { state: bodyState })}</p>
      {isIdentical(comparison) ? <p className={styles.text}>{t('wl.diff.summary.identical')}</p> : null}
      <div className={styles.scroll}>
        <table className={styles.table}>
          <caption className={styles.caption}>{t('wl.diff.summary.table')}</caption>
          <thead>
            <tr>
              <th scope="col">{t('wl.diff.column.field')}</th>
              <th scope="col">
                {t('wl.diff.column.a')} <span className={styles.time}>{leftTime}</span>
              </th>
              <th scope="col">
                {t('wl.diff.column.b')} <span className={styles.time}>{rightTime}</span>
              </th>
              <th scope="col">{t('wl.diff.column.result')}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.label}>
                <th scope="row">{row.label}</th>
                <td className={styles.value}>{row.leftValue === '' ? t('wl.diff.empty') : row.leftValue}</td>
                <td className={styles.value}>{row.rightValue === '' ? t('wl.diff.empty') : row.rightValue}</td>
                <td>
                  <span aria-hidden="true">{row.equal ? '=' : '≠'}</span>{' '}
                  {row.equal ? t('wl.diff.result.same') : t('wl.diff.result.different')}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
