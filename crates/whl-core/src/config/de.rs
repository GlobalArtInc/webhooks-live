use std::fmt::Display;
use std::str::FromStr;
use std::time::Duration;

use bytesize::ByteSize;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
#[serde(untagged)]
enum NumberOrText {
    Number(u64),
    Text(String),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum TextOrItems {
    Text(String),
    Items(Vec<String>),
}

pub fn duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    humantime::parse_duration(&text).map_err(D::Error::custom)
}

pub fn byte_size<'de, D>(deserializer: D) -> Result<ByteSize, D::Error>
where
    D: Deserializer<'de>,
{
    match NumberOrText::deserialize(deserializer)? {
        NumberOrText::Number(bytes) => Ok(ByteSize::b(bytes)),
        NumberOrText::Text(text) => text.parse::<ByteSize>().map_err(D::Error::custom),
    }
}

pub fn list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: Display,
{
    let items = match TextOrItems::deserialize(deserializer)? {
        TextOrItems::Text(text) => text
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
        TextOrItems::Items(items) => items,
    };

    items
        .iter()
        .map(|item| item.parse::<T>().map_err(D::Error::custom))
        .collect()
}
