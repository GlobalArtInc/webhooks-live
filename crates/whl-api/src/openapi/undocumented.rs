pub(super) struct Route {
    pub method: &'static str,
    pub path: &'static str,
}

const fn route(method: &'static str, path: &'static str) -> Route {
    Route { method, path }
}

const ALIASES: &[Route] = &[
    route("GET", "/aliases/{alias}/availability"),
    route("GET", "/endpoints/{id}/aliases"),
    route("DELETE", "/endpoints/{id}/aliases/{alias}"),
    route("PUT", "/endpoints/{id}/aliases/{alias}"),
];

const CAPTURES: &[Route] = &[
    route("DELETE", "/endpoints/{id}/captures"),
    route("GET", "/endpoints/{id}/captures"),
    route("GET", "/endpoints/{id}/captures/latest"),
    route("DELETE", "/endpoints/{id}/captures/{capture_id}"),
    route("GET", "/endpoints/{id}/captures/{capture_id}"),
    route("PATCH", "/endpoints/{id}/captures/{capture_id}"),
    route("GET", "/endpoints/{id}/captures/{capture_id}/body"),
    route("POST", "/endpoints/{id}/captures/{capture_id}/forward"),
    route("GET", "/endpoints/{id}/captures/{capture_id}/parts/{index}"),
    route("POST", "/endpoints/{id}/captures/{capture_id}/resend"),
    route("GET", "/endpoints/{id}/captures/{capture_id}/rule-runs"),
    route("POST", "/endpoints/{id}/captures/{capture_id}/rule-runs"),
    route("GET", "/endpoints/{id}/stats"),
];

const CLONING: &[Route] = &[
    route("POST", "/endpoints/{id}/clone"),
    route("GET", "/templates"),
    route("POST", "/templates"),
    route("DELETE", "/templates/{template_id}"),
    route("GET", "/templates/{template_id}"),
    route("PATCH", "/templates/{template_id}"),
];

const COMPOSER: &[Route] = &[
    route("POST", "/send"),
    route("GET", "/send/history"),
    route("DELETE", "/send/history/{sent_id}"),
];

const ENDPOINTS: &[Route] = &[
    route("GET", "/endpoints"),
    route("POST", "/endpoints"),
    route("DELETE", "/endpoints/{id}"),
    route("GET", "/endpoints/{id}"),
    route("PATCH", "/endpoints/{id}"),
    route("POST", "/endpoints/{id}/claim"),
    route("POST", "/endpoints/{id}/unlock"),
    route("GET", "/instance"),
];

const EXPORT: &[Route] = &[
    route("GET", "/endpoints/{id}/export"),
    route("HEAD", "/endpoints/{id}/export"),
    route("POST", "/endpoints/{id}/export"),
];

const FOLDERS: &[Route] = &[
    route("GET", "/folders"),
    route("POST", "/folders"),
    route("DELETE", "/folders/{folder_id}"),
    route("PATCH", "/folders/{folder_id}"),
    route("DELETE", "/folders/{folder_id}/endpoints/{id}"),
    route("PUT", "/folders/{folder_id}/endpoints/{id}"),
];

const FORWARD: &[Route] = &[
    route("GET", "/endpoints/{id}/forward-rules"),
    route("POST", "/endpoints/{id}/forward-rules"),
    route("DELETE", "/endpoints/{id}/forward-rules/{rule_id}"),
    route("PATCH", "/endpoints/{id}/forward-rules/{rule_id}"),
];

const IMPORT: &[Route] = &[
    route("POST", "/endpoints/import"),
    route("POST", "/endpoints/{id}/import"),
];

const INBOX: &[Route] = &[route("GET", "/inbox/captures")];

const MEMBERS: &[Route] = &[
    route("GET", "/endpoints/{id}/members"),
    route("PUT", "/endpoints/{id}/members"),
    route("DELETE", "/endpoints/{id}/members/{user_id}"),
];

const NOTIFICATIONS: &[Route] = &[
    route("GET", "/email-addresses"),
    route("POST", "/email-addresses"),
    route("POST", "/email-addresses/verify"),
    route("DELETE", "/email-addresses/{address_id}"),
    route("GET", "/endpoints/{id}/notifications"),
    route("POST", "/endpoints/{id}/notifications"),
    route("DELETE", "/endpoints/{id}/notifications/{channel_id}"),
    route("PATCH", "/endpoints/{id}/notifications/{channel_id}"),
    route("POST", "/endpoints/{id}/notifications/{channel_id}/test"),
    route("POST", "/notifications/disable"),
];

const PUSH: &[Route] = &[
    route("GET", "/push/public-key"),
    route("GET", "/push/subscriptions"),
    route("POST", "/push/subscriptions"),
    route("DELETE", "/push/subscriptions/{subscription_id}"),
];

const RULES: &[Route] = &[];

const SCHEDULES: &[Route] = &[
    route("GET", "/endpoints/{id}/schedules"),
    route("POST", "/endpoints/{id}/schedules"),
    route("DELETE", "/endpoints/{id}/schedules/{schedule_id}"),
    route("PATCH", "/endpoints/{id}/schedules/{schedule_id}"),
    route("POST", "/endpoints/{id}/schedules/{schedule_id}/run"),
    route("GET", "/endpoints/{id}/schedules/{schedule_id}/runs"),
];

const SECRETS: &[Route] = &[];

const VARS: &[Route] = &[];

pub(super) fn all() -> Vec<&'static Route> {
    [
        ALIASES,
        CAPTURES,
        CLONING,
        COMPOSER,
        ENDPOINTS,
        EXPORT,
        FOLDERS,
        FORWARD,
        IMPORT,
        INBOX,
        MEMBERS,
        NOTIFICATIONS,
        PUSH,
        RULES,
        SCHEDULES,
        SECRETS,
        VARS,
    ]
    .into_iter()
    .flatten()
    .collect()
}
