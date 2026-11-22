import { useTranslation } from 'react-i18next';
import type { ReferenceParameter } from './reference';
import { SchemaMeta } from './SchemaView';
import { TextBlocks } from './TextBlocks';
import styles from './ParameterTable.module.css';

export interface ParameterTableProps {
  parameters: readonly ReferenceParameter[];
  caption: string;
}

export function ParameterTable({ parameters, caption }: ParameterTableProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.scroll}>
      <table className={styles.table}>
        <caption className={styles.caption}>{caption}</caption>
        <thead>
          <tr>
            <th scope="col">{t('wl.apireference.parameter.name')}</th>
            <th scope="col">{t('wl.apireference.parameter.in')}</th>
            <th scope="col">{t('wl.apireference.parameter.type')}</th>
            <th scope="col">{t('wl.apireference.parameter.description')}</th>
          </tr>
        </thead>
        <tbody>
          {parameters.map((parameter) => (
            <tr key={`${parameter.location}:${parameter.name}`}>
              <th scope="row" className={styles.name}>
                {parameter.name}
                {parameter.required ? (
                  <span className={styles.required}>{t('wl.apireference.schema.required')}</span>
                ) : null}
              </th>
              <td>
                {parameter.location === 'path' ? t('wl.apireference.parameter.in.path') : null}
                {parameter.location === 'query' ? t('wl.apireference.parameter.in.query') : null}
                {parameter.location === 'header' ? t('wl.apireference.parameter.in.header') : null}
                {parameter.location === 'cookie' ? t('wl.apireference.parameter.in.cookie') : null}
              </td>
              <td className={styles.type}>
                {parameter.schema === null ? null : <SchemaMeta node={parameter.schema} />}
              </td>
              <td>
                <TextBlocks text={parameter.description} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
