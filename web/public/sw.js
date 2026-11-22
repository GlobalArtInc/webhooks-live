'use strict';

var ENDPOINT_ID = /^[0-9a-hjkmnp-tv-z]{24}$/;
var CAPTURE_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
var KIND = /^[a-z0-9_-]{1,24}$/;
var CONTROL = /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]+/gu;
var MAX_METHOD = 16;
var MAX_PATH = 100;
var APP_NAME = 'webhooks.live';
var GENERIC_BODY = 'New capture';

function valid(pattern, value) {
  return typeof value === 'string' && pattern.test(value);
}

function clean(value, limit) {
  if (typeof value !== 'string') {
    return '';
  }
  var text = value.replace(CONTROL, ' ').trim();
  var chars = Array.from(text);
  return chars.length > limit ? chars.slice(0, limit).join('') + '…' : text;
}

function readPayload(event) {
  try {
    var payload = event.data ? event.data.json() : null;
    return payload !== null && typeof payload === 'object' && !Array.isArray(payload) ? payload : {};
  } catch (error) {
    return {};
  }
}

function describePush(payload) {
  var title = (clean(payload.method, MAX_METHOD) + ' ' + clean(payload.path, MAX_PATH)).trim();
  var options = {
    body: valid(KIND, payload.kind) ? payload.kind : GENERIC_BODY,
  };
  if (valid(CAPTURE_ID, payload.capture_id)) {
    options.tag = payload.capture_id;
    if (valid(ENDPOINT_ID, payload.endpoint_id)) {
      options.data = { endpoint_id: payload.endpoint_id, capture_id: payload.capture_id };
    }
  }
  return { title: title === '' ? APP_NAME : title, options: options };
}

function targetOf(data) {
  if (
    data !== null &&
    typeof data === 'object' &&
    valid(ENDPOINT_ID, data.endpoint_id) &&
    valid(CAPTURE_ID, data.capture_id)
  ) {
    return '/e/' + data.endpoint_id + '/c/' + data.capture_id;
  }
  return '/';
}

function openTarget(path) {
  var url = new URL(path, self.location.origin).href;
  var fallback = function () {
    return self.clients.openWindow(url);
  };
  return self.clients
    .matchAll({ type: 'window', includeUncontrolled: true })
    .then(function (windows) {
      var own = windows.filter(function (client) {
        return new URL(client.url).origin === self.location.origin;
      });
      var target =
        own.find(function (client) {
          return client.visibilityState === 'visible';
        }) || own[0];
      if (!target) {
        return fallback();
      }
      return target.focus().then(function (focused) {
        if (focused && typeof focused.navigate === 'function') {
          return focused.navigate(url);
        }
        return fallback();
      });
    })
    .catch(fallback);
}

self.addEventListener('install', function () {
  self.skipWaiting();
});

self.addEventListener('activate', function (event) {
  event.waitUntil(self.clients.claim());
});

self.addEventListener('push', function (event) {
  var shown = describePush(readPayload(event));
  event.waitUntil(self.registration.showNotification(shown.title, shown.options));
});

self.addEventListener('notificationclick', function (event) {
  event.notification.close();
  event.waitUntil(openTarget(targetOf(event.notification.data)));
});
