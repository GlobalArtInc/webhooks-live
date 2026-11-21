import { useId, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { CodeBlock } from './CodeBlock';
import { DEFAULT_PORT, buildForwardCommand, isValidPrefix, parsePort, type StartPoint } from './command';
import styles from './CommandBuilder.module.css';

const START_POINTS: readonly StartPoint[] = ['resume', 'now', 'all'];

export interface CommandBuilderProps {
  endpointUrl: string;
}

export function CommandBuilder({ endpointUrl }: CommandBuilderProps) {
  const { t } = useTranslation();
  const baseId = useId();
  const [portText, setPortText] = useState(String(DEFAULT_PORT));
  const [prefix, setPrefix] = useState('');
  const [start, setStart] = useState<StartPoint>('resume');
  const [dryRun, setDryRun] = useState(false);

  const port = parsePort(portText);
  const prefixValid = isValidPrefix(prefix);
  const command =
    port !== null && prefixValid ? buildForwardCommand({ endpointUrl, port, prefix, start, dryRun }) : null;

  const portId = `${baseId}-port`;
  const portHintId = `${baseId}-port-hint`;
  const prefixId = `${baseId}-prefix`;
  const prefixHintId = `${baseId}-prefix-hint`;

  function startLabel(point: StartPoint): string {
    switch (point) {
      case 'resume':
        return t('wl.cli.run.start.resume');
      case 'now':
        return t('wl.cli.run.start.now');
      case 'all':
        return t('wl.cli.run.start.all');
    }
  }

  return (
    <div className={styles.form}>
      <div className={styles.fields}>
        <div className={styles.field}>
          <label className={styles.label} htmlFor={portId}>
            {t('wl.cli.run.port.label')}
          </label>
          <input
            id={portId}
            className={styles.input}
            type="text"
            inputMode="numeric"
            autoComplete="off"
            value={portText}
            aria-invalid={port === null}
            aria-describedby={portHintId}
            onChange={(event) => {
              setPortText(event.target.value);
            }}
          />
          <p id={portHintId} className={port === null ? styles.error : styles.hint}>
            {port === null ? t('wl.cli.run.port.invalid') : t('wl.cli.run.port.hint')}
          </p>
        </div>
        <div className={styles.field}>
          <label className={styles.label} htmlFor={prefixId}>
            {t('wl.cli.run.prefix.label')}
          </label>
          <input
            id={prefixId}
            className={styles.input}
            type="text"
            autoComplete="off"
            value={prefix}
            aria-invalid={!prefixValid}
            aria-describedby={prefixHintId}
            onChange={(event) => {
              setPrefix(event.target.value);
            }}
          />
          <p id={prefixHintId} className={prefixValid ? styles.hint : styles.error}>
            {prefixValid ? t('wl.cli.run.prefix.hint') : t('wl.cli.run.prefix.invalid')}
          </p>
        </div>
      </div>
      <fieldset className={styles.fieldset}>
        <legend className={styles.legend}>{t('wl.cli.run.start.legend')}</legend>
        {START_POINTS.map((point) => (
          <label key={point} className={styles.option}>
            <input
              type="radio"
              name={`${baseId}-start`}
              value={point}
              checked={start === point}
              onChange={() => {
                setStart(point);
              }}
            />
            {startLabel(point)}
          </label>
        ))}
      </fieldset>
      <label className={styles.option}>
        <input
          type="checkbox"
          checked={dryRun}
          onChange={(event) => {
            setDryRun(event.target.checked);
          }}
        />
        {t('wl.cli.run.dryRun')}
      </label>
      {command === null ? (
        <p role="status" className={styles.error}>
          {t('wl.cli.run.fixFields')}
        </p>
      ) : (
        <CodeBlock text={command} label={t('wl.cli.run.command.label')} copyLabel={t('wl.cli.run.command.copy')} />
      )}
      <p className={styles.hint}>{t('wl.cli.run.hint')}</p>
    </div>
  );
}
