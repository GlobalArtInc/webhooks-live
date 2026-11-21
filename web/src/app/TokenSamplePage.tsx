import { useTranslation } from 'react-i18next';
import { MethodBadge } from '../components/MethodBadge/MethodBadge';
import { LanguageControl } from '../i18n/LanguageControl';
import { ThemeControl } from './ThemeControl';
import styles from './TokenSamplePage.module.css';

interface ColourSample {
  role: string;
  token: string;
}

interface TypeSample {
  token: string;
  use: string;
  sample: string;
}

interface StatusSample {
  token: string;
  label: string;
}

interface NamedSample {
  token: string;
  note: string;
}

interface MotionSample extends NamedSample {
  easing?: string;
}

const COLOUR_SAMPLES: ColourSample[] = [
  { role: 'bg canvas', token: 'color-bg' },
  { role: 'surface panels', token: 'color-surface' },
  { role: 'surface-raised sheets, menus', token: 'color-surface-raised' },
  { role: 'surface-sunken code', token: 'color-surface-sunken' },
  { role: 'border hairline', token: 'color-border' },
  { role: 'border-strong inputs', token: 'color-border-strong' },
  { role: 'text', token: 'color-text' },
  { role: 'text-muted', token: 'color-text-muted' },
  { role: 'accent fill', token: 'color-accent' },
  { role: 'text-on-accent', token: 'color-text-on-accent' },
  { role: 'accent-text links, active', token: 'color-accent-text' },
  { role: 'accent-soft selection, arrival', token: 'color-accent-soft' },
  { role: 'focus ring', token: 'color-focus' },
  { role: 'success', token: 'color-success' },
  { role: 'warning', token: 'color-warning' },
  { role: 'danger', token: 'color-danger' },
  { role: 'info', token: 'color-info' },
];

const TYPE_SAMPLES: TypeSample[] = [
  { token: 'text-meta', use: 'timestamps, sizes, hints, badges, table headers', sample: 'last 12 s ago' },
  { token: 'text-data', use: 'default UI text, labels, buttons', sample: 'Create endpoint' },
  {
    token: 'mono-data',
    use: 'paths, URLs, headers, bodies, ids',
    sample: 'https://hooks.example/k7Qm2xTa9vNc',
  },
  { token: 'text-section', use: 'section titles', sample: 'My endpoints' },
  { token: 'text-title', use: 'page titles, capture URL in header', sample: 'stripe-dev' },
  { token: 'text-hero', use: 'home headline, empty-state URL', sample: 'Watch HTTP requests arrive live.' },
];

const METHOD_SAMPLES = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS', 'HEAD', 'TRACE'];

const PLURAL_SAMPLE_COUNTS = [1, 2, 5];

const HTTP_STATUS_SAMPLES: StatusSample[] = [
  { token: 'color-success', label: '2xx' },
  { token: 'color-info', label: '3xx' },
  { token: 'color-warning', label: '4xx' },
  { token: 'color-danger', label: '5xx' },
];

const FORWARD_RESULT_SAMPLES: StatusSample[] = [
  { token: 'color-success', label: 'ok' },
  { token: 'color-danger', label: 'failed' },
  { token: 'color-warning', label: 'refused by SSRF guard' },
  { token: 'color-text-muted', label: 'pending' },
];

const SPACE_TOKENS = ['space-0', 'space-1', 'space-2', 'space-3', 'space-4', 'space-5', 'space-6', 'space-7'];

const RADIUS_SAMPLES: NamedSample[] = [
  { token: 'radius-sm', note: 'badges, inputs' },
  { token: 'radius-md', note: 'buttons, chips' },
  { token: 'radius-lg', note: 'sheets, dialogs, toasts' },
  { token: 'radius-pill', note: 'pills' },
];

const ELEVATION_SAMPLES: NamedSample[] = [
  { token: 'elevation-0', note: 'flat, panels separated by border' },
  { token: 'elevation-1', note: 'menus, popovers' },
  { token: 'elevation-2', note: 'sheets' },
  { token: 'elevation-3', note: 'dialogs, toasts' },
];

const MOTION_SAMPLES: MotionSample[] = [
  { token: 'duration-fast', note: 'hover, press' },
  { token: 'duration-base', note: 'menus, tabs, pill' },
  { token: 'duration-sheet', note: 'sheets, drawers', easing: 'var(--ease-sheet)' },
  { token: 'duration-arrival', note: 'arrival fade', easing: 'var(--ease-arrival)' },
];

export function TokenSamplePage() {
  const { t } = useTranslation();

  return (
    <main className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>{t('wl.sample.title')}</h1>
        <LanguageControl />
        <ThemeControl />
      </header>

      <section className={styles.section}>
        <h2 className={styles.heading}>Colour</h2>
        <ul className={styles.swatches}>
          {COLOUR_SAMPLES.map((sample) => (
            <li key={sample.token} className={styles.swatch}>
              <span className={styles.chip} style={{ background: `var(--${sample.token})` }} aria-hidden="true" />
              <span className={styles.swatchText}>
                <span>{sample.role}</span>
                <code className={styles.token}>--{sample.token}</code>
              </span>
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>Type</h2>
        <ul className={styles.list}>
          {TYPE_SAMPLES.map((sample) => (
            <li key={sample.token} className={styles.typeRow}>
              <span className={styles.caption}>
                <code className={styles.token}>--{sample.token}</code> {sample.use}
              </span>
              <span style={{ font: `var(--${sample.token})` }}>{sample.sample}</span>
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>Method badges</h2>
        <ul className={styles.row}>
          {METHOD_SAMPLES.map((method) => (
            <li key={method}>
              <MethodBadge method={method} />
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>Status</h2>
        <p className={styles.caption}>{t('wl.state.gone.title')}</p>
        <p className={styles.caption}>HTTP status</p>
        <ul className={styles.row}>
          {HTTP_STATUS_SAMPLES.map((sample) => (
            <li key={sample.label} className={styles.status} style={{ color: `var(--${sample.token})` }}>
              {sample.label}
            </li>
          ))}
        </ul>
        <p className={styles.caption}>Forward result</p>
        <ul className={styles.row}>
          {FORWARD_RESULT_SAMPLES.map((sample) => (
            <li key={sample.label} className={styles.status} style={{ color: `var(--${sample.token})` }}>
              {sample.label}
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>{t('wl.sample.plurals.heading')}</h2>
        <ul className={styles.list}>
          {PLURAL_SAMPLE_COUNTS.map((count) => (
            <li key={count} className={styles.caption}>
              {t('wl.sample.newCaptures', { count })}
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>Focus and buttons</h2>
        <div className={styles.row}>
          <button type="button" className={styles.button}>
            {t('wl.common.createEndpoint')}
          </button>
          <span className={styles.caption}>Focus ring: 2 px, 2 px offset, Tab to see it</span>
        </div>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>Spacing</h2>
        <ul className={styles.list}>
          {SPACE_TOKENS.map((token) => (
            <li key={token} className={styles.spaceRow}>
              <code className={styles.token}>--{token}</code>
              <span className={styles.spaceBar} style={{ width: `var(--${token})` }} />
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>Radius</h2>
        <ul className={styles.samples}>
          {RADIUS_SAMPLES.map((sample) => (
            <li key={sample.token} className={styles.sample}>
              <span className={styles.box} style={{ borderRadius: `var(--${sample.token})` }} />
              <code className={styles.token}>--{sample.token}</code>
              <span className={styles.caption}>{sample.note}</span>
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>Elevation</h2>
        <ul className={styles.samples}>
          {ELEVATION_SAMPLES.map((sample) => (
            <li key={sample.token} className={styles.sample}>
              <span className={styles.box} style={{ boxShadow: `var(--${sample.token})` }} />
              <code className={styles.token}>--{sample.token}</code>
              <span className={styles.caption}>{sample.note}</span>
            </li>
          ))}
        </ul>
      </section>

      <section className={styles.section}>
        <h2 className={styles.heading}>Motion</h2>
        <p className={styles.caption}>Hover each box to play its transition</p>
        <ul className={styles.samples}>
          {MOTION_SAMPLES.map((sample) => (
            <li key={sample.token} className={styles.sample}>
              <span
                className={`${styles.box} ${styles.motion}`}
                style={{ transitionDuration: `var(--${sample.token})`, transitionTimingFunction: sample.easing }}
              />
              <code className={styles.token}>--{sample.token}</code>
              <span className={styles.caption}>{sample.note}</span>
            </li>
          ))}
        </ul>
      </section>
    </main>
  );
}
