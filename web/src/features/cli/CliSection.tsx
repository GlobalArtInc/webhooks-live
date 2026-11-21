import { Link } from '@tanstack/react-router';
import { useId, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { useEndpoint, useInstance } from '../../api/queries';
import { showsTokensLink } from '../tokens/TokensLink';
import type { SectionProps } from '../workspace/sections';
import { CodeBlock } from './CodeBlock';
import { CommandBuilder } from './CommandBuilder';
import { InstallTabs } from './InstallTabs';
import styles from './CliSection.module.css';

const TOKEN_EXPORT_LINE = 'export WEBHOOKS_LIVE_TOKEN=<token>';

interface SubSectionProps {
  title: string;
  children: ReactNode;
}

function SubSection({ title, children }: SubSectionProps) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className={styles.block}>
      <h3 id={headingId} className={styles.subheading}>
        {title}
      </h3>
      {children}
    </section>
  );
}

export function CliSection({ endpointId }: SectionProps) {
  const { t } = useTranslation();
  const endpointQuery = useEndpoint(endpointId);
  const endpoint = endpointQuery.data;
  const instance = useInstance().data;
  const tokensAvailable = instance !== undefined && showsTokensLink(instance);

  return (
    <div className={styles.page}>
      <h2 className={styles.heading}>{t('wl.workspace.section.cli')}</h2>
      <p className={styles.text}>{t('wl.cli.intro')}</p>
      <SubSection title={t('wl.cli.install.title')}>
        <InstallTabs />
      </SubSection>
      <SubSection title={t('wl.cli.token.title')}>
        <p className={styles.text}>{t('wl.cli.token.body')}</p>
        {tokensAvailable ? (
          <Link to="/settings/tokens" className={styles.link}>
            {t('wl.cli.token.create')}
          </Link>
        ) : null}
        <p className={styles.text}>{t('wl.cli.token.env')}</p>
        <CodeBlock text={TOKEN_EXPORT_LINE} label={t('wl.cli.token.exportLine')} />
        <p className={styles.text}>{t('wl.cli.token.login')}</p>
      </SubSection>
      <SubSection title={t('wl.cli.run.title')}>
        <p className={styles.text}>{t('wl.cli.run.intro')}</p>
        {endpoint === undefined ? (
          endpointQuery.isError ? (
            <p role="alert" className={styles.error}>
              {t('wl.cli.run.error')}
            </p>
          ) : (
            <p role="status" className={styles.text}>
              {t('wl.workspace.loading')}
            </p>
          )
        ) : (
          <CommandBuilder endpointUrl={endpoint.url} />
        )}
      </SubSection>
      <SubSection title={t('wl.cli.warning.title')}>
        <p className={styles.warning}>{t('wl.cli.warning.body')}</p>
      </SubSection>
      <SubSection title={t('wl.cli.trouble.title')}>
        <ul className={styles.list}>
          <li>{t('wl.cli.trouble.exit3')}</li>
          <li>{t('wl.cli.trouble.exit4')}</li>
          <li>{t('wl.cli.trouble.exit5')}</li>
          <li>{t('wl.cli.trouble.target')}</li>
          <li>{t('wl.cli.trouble.path')}</li>
          <li>{t('wl.cli.trouble.repeat')}</li>
        </ul>
      </SubSection>
    </div>
  );
}
