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
        // Too large fails.
        let len = value.len();
        GermanString::new_from_str_fallible(value).map_err(|_| E::invalid_length(len, &self))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        // Too large fails.
        let len = value.len();
        GermanString::new_fallible(value).map_err(|_| E::invalid_length(len, &self))
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

#[cfg(test)]
mod test {
    use super::GermanString;
    use crate::test_util::{all_strings, gs};

    #[test]
    fn serializes_as_plain_json_string() {
        for s in all_strings() {
            assert_eq!(
                serde_json::to_string(&gs(s)).unwrap(),
                serde_json::to_string(s).unwrap(),
                "{s:?}"
            );
        }
    }

    #[test]
    fn serde_json_round_trips() {
        for s in all_strings() {
            let json = serde_json::to_string(&gs(s)).unwrap();
            let back: GermanString = serde_json::from_str(&json).unwrap();
            assert!(back == gs(s), "{s:?}");
            assert_eq!(back.as_str(), s, "{s:?}");
        }
    }

    #[test]
    fn deserializes_owned_and_escaped_strings() {
        // Escapes force serde_json to hand over an owned/unescaped buffer instead of a borrow.
        let back: GermanString =
            serde_json::from_str(r#""tab\there, a long escaped string""#).unwrap();
        assert_eq!(back.as_str(), "tab\there, a long escaped string");

        let back: GermanString = serde_json::from_str(r#""\u00e9t\u00e9""#).unwrap();
        assert_eq!(back.as_str(), "été");
    }

    #[test]
    fn deserializes_inside_containers() {
        let words = all_strings();
        let json = serde_json::to_string(&words).unwrap();
        let back: Vec<GermanString> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.len(), words.len());
        for (g, s) in back.iter().zip(&words) {
            assert_eq!(g.as_str(), *s);
        }
    }

    #[test]
    fn deserialize_rejects_non_strings() {
        for json in ["42", "null", "true", "[]", "{}"] {
            assert!(
                serde_json::from_str::<GermanString>(json).is_err(),
                "{json} should not deserialize"
            );
        }
    }

    #[test]
    fn deserialize_rejects_invalid_utf8_bytes() {
        use serde::Deserialize;
        use serde::de::value::{BytesDeserializer, Error};

        // A format that hands over raw bytes must never produce a GermanString from invalid UTF-8.
        for bytes in [
            &b"\xff"[..],
            b"\xfe\xff",
            b"abcdefghijklmnop\xff",
            b"abc\xc3",
        ] {
            let result = GermanString::deserialize(BytesDeserializer::<Error>::new(bytes));
            assert!(result.is_err(), "{bytes:?} should be rejected");
        }
    }
}
