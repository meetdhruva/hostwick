use std::collections::HashMap;

/// A registry-style value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Str(String),
    Dword(u32),
}

/// One saved session as key/value pairs, from a `.reg` file, an `.ini`
/// file, or the live Windows registry. Keys are lowercased.
#[derive(Debug, Default, Clone)]
pub(crate) struct RawSession {
    pub name: String,
    pub values: HashMap<String, Value>,
}

impl RawSession {
    pub fn str(&self, key: &str) -> Option<String> {
        match self.values.get(&key.to_ascii_lowercase())? {
            Value::Str(s) if !s.is_empty() => Some(s.clone()),
            Value::Str(_) => None,
            Value::Dword(d) => Some(d.to_string()),
        }
    }

    pub fn num(&self, key: &str) -> Option<u32> {
        match self.values.get(&key.to_ascii_lowercase())? {
            Value::Dword(d) => Some(*d),
            Value::Str(s) => s.trim().parse().ok(),
        }
    }

    pub fn insert(&mut self, key: &str, v: Value) {
        self.values.insert(key.to_ascii_lowercase(), v);
    }
}
