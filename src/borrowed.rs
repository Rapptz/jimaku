//! A special Cow<'de, str> wrapper type for no allocation serde deserialization.
//!
//! This type only exists because of a serde limitation:
//!
//! https://github.com/serde-rs/serde/issues/1852

use std::borrow::Cow;

use serde::{de::Visitor, Deserialize, Serialize};

pub struct MaybeBorrowedString<'a>(pub Cow<'a, str>);

impl<'a> std::ops::Deref for MaybeBorrowedString<'a> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a> MaybeBorrowedString<'a> {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for MaybeBorrowedString<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(MaybeBorrowedStringVisitor)
    }
}

impl<'a> Serialize for MaybeBorrowedString<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

struct MaybeBorrowedStringVisitor;

impl<'de> Visitor<'de> for MaybeBorrowedStringVisitor {
    type Value = MaybeBorrowedString<'de>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string")
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(MaybeBorrowedString(Cow::Borrowed(v)))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(MaybeBorrowedString(Cow::Owned(v.to_owned())))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(MaybeBorrowedString(Cow::Owned(v)))
    }
}
