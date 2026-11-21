import { describe, expect, it } from 'vitest';
import { slackChannelFixture, emailChannelFixture, VERIFIED_ADDRESS_ID } from '../../test/fixtures/notifications';
import { blankDraft, channelInputOf, draftOf, issuesFromServer, validateDraft } from './channelDraft';

describe('channel draft', () => {
  it('requires a URL for a new webhook but not for an existing channel whose URL stays masked', () => {
    expect(validateDraft(blankDraft('webhook'), true).get('url')).toBe('required');
    expect(validateDraft(draftOf(slackChannelFixture), false).has('url')).toBe(false);
  });

  it('sends the config only when the URL is typed, so the masked URL is never sent back', () => {
    const unchanged = channelInputOf({ ...draftOf(slackChannelFixture), name: 'ops-room' }, false);
    expect(unchanged).toEqual({ name: 'ops-room', per_hour: 60 });

    const typed = channelInputOf({ ...draftOf(slackChannelFixture), url: 'https://hooks.example.test/ops' }, false);
    expect(typed.config).toEqual({ url: 'https://hooks.example.test/ops' });
  });

  it('builds the new telegram and email inputs and rejects a limit below one', () => {
    const telegram = { ...blankDraft('telegram'), name: 'bot', tokenSecret: 'TELEGRAM_TOKEN', chatId: ' 42 ' };
    expect(channelInputOf(telegram, true)).toEqual({
      kind: 'telegram',
      name: 'bot',
      per_hour: 60,
      trigger: 'capture',
      enabled: true,
      config: { token_secret: 'TELEGRAM_TOKEN', chat_id: '42' },
    });

    const email = { ...draftOf(emailChannelFixture), perHour: '0' };
    expect(channelInputOf(email, false).config).toEqual({ address_id: VERIFIED_ADDRESS_ID });
    expect(validateDraft(email, false).get('perHour')).toBe('perHour');
  });

  it('maps server field errors to the fields and their issue codes', () => {
    const issues = issuesFromServer([{ field: 'config.url', code: 'target_refused', message: 'refused' }]);
    expect(issues.get('url')).toBe('targetRefused');
  });
});
