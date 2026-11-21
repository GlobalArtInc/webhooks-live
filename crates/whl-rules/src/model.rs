use serde::{Serialize, Serializer, ser::SerializeMap};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rule {
    pub name: String,
    pub enabled: bool,
    pub condition: Condition,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Always,
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    Leaf(Leaf),
}

impl Serialize for Condition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Always => serializer.serialize_map(Some(0))?.end(),
            Self::All(items) => single_entry(serializer, "all", items),
            Self::Any(items) => single_entry(serializer, "any", items),
            Self::Not(inner) => single_entry(serializer, "not", inner.as_ref()),
            Self::Leaf(leaf) => leaf.serialize(serializer),
        }
    }
}

fn single_entry<S, T>(serializer: S, key: &str, value: &T) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize + ?Sized,
{
    let mut map = serializer.serialize_map(Some(1))?;
    map.serialize_entry(key, value)?;
    map.end()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Leaf {
    #[serde(rename = "value")]
    pub path: String,
    pub op: Op,
    #[serde(rename = "arg")]
    pub argument: Option<Argument>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Eq,
    Ne,
    Contains,
    StartsWith,
    EndsWith,
    Matches,
    Exists,
    Gt,
    Lt,
    In,
    InCidr,
}

impl Op {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "eq" => Self::Eq,
            "ne" => Self::Ne,
            "contains" => Self::Contains,
            "starts_with" => Self::StartsWith,
            "ends_with" => Self::EndsWith,
            "matches" => Self::Matches,
            "exists" => Self::Exists,
            "gt" => Self::Gt,
            "lt" => Self::Lt,
            "in" => Self::In,
            "in_cidr" => Self::InCidr,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Argument {
    Text(String),
    Number(serde_json::Number),
    List(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Step {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub continue_on_error: bool,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BodySource {
    Capture,
    Payload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum S3Content {
    CaptureJson,
    Body,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Respond {
        status: String,
        content_type: Option<String>,
        headers: Vec<Header>,
        body: String,
    },
    SetHeader {
        name: String,
        value: String,
    },
    RemoveHeader {
        name: String,
    },
    Delay {
        ms: u32,
    },
    Stop,
    Extract {
        from: String,
        json_path: Option<String>,
        regex: Option<String>,
        group: Option<u32>,
        into: String,
    },
    SetVar {
        into: String,
        value: String,
    },
    Transform {
        into: String,
        template: String,
    },
    SetStore {
        name: String,
        value: String,
    },
    Increment {
        counter: String,
        by: i64,
    },
    Script {
        source: String,
    },
    Http {
        method: String,
        url: String,
        headers: Vec<Header>,
        body: Option<String>,
        timeout_ms: u32,
        follow_redirects: bool,
    },
    Forward {
        forward_rule_id: Option<Uuid>,
        target_url: Option<String>,
        method: Option<String>,
        content_type: Option<String>,
        header_allowlist: Vec<String>,
        body_from: BodySource,
    },
    Notify {
        channel_id: Uuid,
        message: Option<String>,
    },
    StoreS3 {
        bucket_url: String,
        prefix: String,
        region: String,
        access_key_secret: String,
        secret_key_secret: String,
        content: S3Content,
    },
}
