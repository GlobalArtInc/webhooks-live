import { useId, useRef, useState, type KeyboardEvent } from 'react';
import { useTranslation } from 'react-i18next';
import { CodeBlock } from './CodeBlock';
import {
  CLI_PLATFORMS,
  CLI_RELEASE_VERSION,
  archiveName,
  checksumsName,
  installCommand,
  releaseAssetUrl,
  sourceInstallCommand,
  verifyCommand,
  type CliPlatform,
} from './release';
import styles from './InstallTabs.module.css';

type InstallTabId = CliPlatform['id'] | 'source';

const TAB_IDS: readonly InstallTabId[] = [...CLI_PLATFORMS.map((platform) => platform.id), 'source'];

interface PlatformPanelProps {
  platform: CliPlatform;
}

function platformOf(id: InstallTabId): CliPlatform | undefined {
  return CLI_PLATFORMS.find((platform) => platform.id === id);
}

function PlatformPanel({ platform }: PlatformPanelProps) {
  const { t } = useTranslation();
  const verify = verifyCommand(platform);
  const install = installCommand(platform);
  return (
    <>
      <p className={styles.text}>{t('wl.cli.install.release', { version: CLI_RELEASE_VERSION })}</p>
      <ul className={styles.links}>
        <li>
          <a className={styles.link} href={releaseAssetUrl(archiveName(platform))}>
            {t('wl.cli.install.archive')}
          </a>
        </li>
        <li>
          <a className={styles.link} href={releaseAssetUrl(checksumsName())}>
            {t('wl.cli.install.checksums')}
          </a>
        </li>
      </ul>
      <p className={styles.text}>{t('wl.cli.install.verifyStep')}</p>
      <CodeBlock text={verify} label={t('wl.cli.install.verifyStep')} copyLabel={t('wl.cli.install.copyVerify')} />
      <p className={styles.text}>{t('wl.cli.install.installStep')}</p>
      <CodeBlock text={install} label={t('wl.cli.install.installStep')} copyLabel={t('wl.cli.install.copyInstall')} />
    </>
  );
}

export function InstallTabs() {
  const { t } = useTranslation();
  const baseId = useId();
  const [selected, setSelected] = useState<InstallTabId>('linuxAmd64');
  const tabRefs = useRef(new Map<InstallTabId, HTMLButtonElement>());

  function labelOf(id: InstallTabId): string {
    switch (id) {
      case 'linuxAmd64':
        return t('wl.cli.install.tab.linuxAmd64');
      case 'linuxArm64':
        return t('wl.cli.install.tab.linuxArm64');
      case 'macosArm64':
        return t('wl.cli.install.tab.macosArm64');
      case 'macosAmd64':
        return t('wl.cli.install.tab.macosAmd64');
      case 'source':
        return t('wl.cli.install.tab.source');
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number): void {
    if (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft') {
      return;
    }
    event.preventDefault();
    const step = event.key === 'ArrowRight' ? 1 : TAB_IDS.length - 1;
    const next = TAB_IDS[(index + step) % TAB_IDS.length];
    if (next !== undefined) {
      setSelected(next);
      tabRefs.current.get(next)?.focus();
    }
  }

  return (
    <div className={styles.root}>
      <div role="tablist" aria-label={t('wl.cli.install.tablist')} className={styles.tabs}>
        {TAB_IDS.map((id, index) => (
          <button
            key={id}
            type="button"
            role="tab"
            id={`${baseId}-${id}-tab`}
            aria-selected={selected === id}
            aria-controls={`${baseId}-${id}-panel`}
            tabIndex={selected === id ? 0 : -1}
            className={styles.tab}
            ref={(element) => {
              if (element === null) {
                tabRefs.current.delete(id);
              } else {
                tabRefs.current.set(id, element);
              }
            }}
            onClick={() => {
              setSelected(id);
            }}
            onKeyDown={(event) => {
              handleKeyDown(event, index);
            }}
          >
            {labelOf(id)}
          </button>
        ))}
      </div>
      {TAB_IDS.map((id) => {
        const platform = platformOf(id);
        return (
          <div
            key={id}
            role="tabpanel"
            id={`${baseId}-${id}-panel`}
            aria-labelledby={`${baseId}-${id}-tab`}
            hidden={selected !== id}
            className={styles.panel}
          >
            {platform === undefined ? (
              <>
                <p className={styles.text}>{t('wl.cli.install.source.body')}</p>
                <CodeBlock
                  text={sourceInstallCommand()}
                  label={t('wl.cli.install.tab.source')}
                  copyLabel={t('wl.cli.install.source.copy')}
                />
              </>
            ) : (
              <PlatformPanel platform={platform} />
            )}
          </div>
        );
      })}
    </div>
  );
}
