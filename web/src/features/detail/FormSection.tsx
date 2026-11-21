import { useTranslation } from 'react-i18next';
import type { CaptureForm } from '../../api/types';
import { KeyValueTable } from '../../components/KeyValueTable/KeyValueTable';
import styles from './CaptureDetail.module.css';
import { formatByteSize } from '../../components/format/format';
import type { FormFileView } from './types';

const EMPTY_CELL = '—';

interface FormSectionProps {
  form: CaptureForm;
  locale: string;
  onDownloadFile: (file: FormFileView) => void;
}

export function FormSection({ form, locale, onDownloadFile }: FormSectionProps) {
  const { t } = useTranslation();
  return (
    <div className={styles.stack}>
      <h3 className={styles.subheading}>{t('wl.detail.form.fields')}</h3>
      <KeyValueTable label={t('wl.detail.form.fields')} rows={form.fields} />
      {form.files.length === 0 ? null : (
        <>
          <h3 className={styles.subheading}>{t('wl.detail.form.files')}</h3>
          <div className={styles.scroller}>
            <table aria-label={t('wl.detail.form.files')} className={styles.dataTable}>
              <thead>
                <tr>
                  <th scope="col">{t('wl.detail.form.file.name')}</th>
                  <th scope="col">{t('wl.detail.form.file.filename')}</th>
                  <th scope="col">{t('wl.detail.form.file.type')}</th>
                  <th scope="col">{t('wl.detail.form.file.size')}</th>
                  <th scope="col" />
                </tr>
              </thead>
              <tbody>
                {form.files.map((file, index) => (
                  <tr key={index}>
                    <th scope="row" className={styles.mono}>
                      {file.name}
                    </th>
                    <td className={styles.mono}>{file.filename ?? EMPTY_CELL}</td>
                    <td className={styles.mono}>{file.content_type ?? EMPTY_CELL}</td>
                    <td>{formatByteSize(file.size, locale)}</td>
                    <td>
                      <button
                        type="button"
                        className={styles.action}
                        aria-label={t('wl.detail.form.downloadNamed', { name: file.name })}
                        onClick={() => {
                          onDownloadFile(file);
                        }}
                      >
                        {t('wl.detail.form.download')}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  );
}
