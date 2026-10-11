use crate::core::GermanString;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Visitor},
};
use std::fmt;

struct GermanStringVisitor;

impl Visitor<'_> for GermanStringVisitor {
    type Value = GermanString;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Expected a valid utf-8 encoded string with length less than {}",
            u32::MAX
        )
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        let gs = GermanString::new_from_str_fallible(value)
            .map_err(|_| E::invalid_length(value.len(), &self))?;
        Ok(gs)
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(GermanString::new_from_string(value))
    }
}

impl Serialize for GermanString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GermanString {
    // Requires string to be utf8.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(GermanStringVisitor)
    }
}
