import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { isFollowing, reduceFollow, type FollowState } from './follow';
import { dispatchFollow, parseLivePrefs, resetLivePrefsState, saveLivePrefs, DEFAULT_LIVE_PREFS } from './prefs';
import { PREFS_STORAGE_KEY } from '../../prefs/theme';

const OFF: FollowState = { enabled: false, paused: false };
const ON: FollowState = { enabled: true, paused: false };
const PAUSED: FollowState = { enabled: true, paused: true };

describe('follow rules', () => {
  it('is off by default and only follows when enabled and not paused', () => {
    expect(isFollowing(OFF)).toBe(false);
    expect(isFollowing(ON)).toBe(true);
    expect(isFollowing(PAUSED)).toBe(false);
  });

  it('toggles on and off, and a toggle while paused resumes instead of switching off', () => {
    expect(reduceFollow(OFF, { kind: 'toggle' })).toEqual(ON);
    expect(reduceFollow(ON, { kind: 'toggle' })).toEqual(OFF);
    expect(reduceFollow(PAUSED, { kind: 'toggle' })).toEqual(ON);
  });

  it('pauses when an older capture is selected by hand', () => {
    expect(reduceFollow(ON, { kind: 'select', captureId: 'old', newestId: 'new' })).toEqual(PAUSED);
  });

  it('does not pause when the newest capture is selected or follow is not running', () => {
    expect(reduceFollow(ON, { kind: 'select', captureId: 'new', newestId: 'new' })).toBe(ON);
    expect(reduceFollow(OFF, { kind: 'select', captureId: 'old', newestId: 'new' })).toBe(OFF);
    expect(reduceFollow(PAUSED, { kind: 'select', captureId: 'old', newestId: 'new' })).toBe(PAUSED);
  });

  it('resumes only a paused follow and never switches a disabled one on', () => {
    expect(reduceFollow(PAUSED, { kind: 'resume' })).toEqual(ON);
    expect(reduceFollow(OFF, { kind: 'resume' })).toBe(OFF);
    expect(reduceFollow(ON, { kind: 'resume' })).toBe(ON);
  });
});

describe('live preferences', () => {
  beforeEach(() => {
    window.localStorage.clear();
    resetLivePrefsState();
  });

  afterEach(() => {
    window.localStorage.clear();
    resetLivePrefsState();
  });

  it('falls back to defaults for missing or damaged storage', () => {
    expect(parseLivePrefs(null)).toEqual(DEFAULT_LIVE_PREFS);
    expect(parseLivePrefs('{not json')).toEqual(DEFAULT_LIVE_PREFS);
    expect(parseLivePrefs('{"follow":"yes","announce":1}')).toEqual(DEFAULT_LIVE_PREFS);
    expect(parseLivePrefs('{"follow":true,"announce":false}')).toEqual({ follow: true, announce: false });
  });

  it('remembers the follow switch but not the pause, next to other preferences', () => {
    window.localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify({ theme: 'dark' }));
    dispatchFollow({ kind: 'toggle' });
    dispatchFollow({ kind: 'select', captureId: 'old', newestId: 'new' });

    expect(JSON.parse(window.localStorage.getItem(PREFS_STORAGE_KEY) ?? '{}')).toEqual({ theme: 'dark', follow: true });
    resetLivePrefsState();
    expect(parseLivePrefs(window.localStorage.getItem(PREFS_STORAGE_KEY)).follow).toBe(true);
  });

  it('stores the mute preference', () => {
    saveLivePrefs({ announce: false });

    expect(parseLivePrefs(window.localStorage.getItem(PREFS_STORAGE_KEY)).announce).toBe(false);
  });
});
