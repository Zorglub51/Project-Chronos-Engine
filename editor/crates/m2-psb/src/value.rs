use indexmap::IndexMap;

/// In-memory PSB value tree. Object key order is preserved (matches Python
/// reader behaviour, which relies on insertion order).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    Object(IndexMap<String, Value>),
    /// Indexed binary blob (textures, audio, raw bytes). Index identifies the
    /// blob within the file's stream table; data is the raw bytes.
    Stream(Stream),
    /// Same as Stream but stored in the v4-only "bstream" table.
    BStream(Stream),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub index: u32,
    pub data: Vec<u8>,
}

impl Value {
    /// Convert to a serde_json::Value, replacing streams with the
    /// `"_stream:N"` / `"_bstream:N"` string form (matches `PsbJsonEncoder`).
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Value::Null => serde_json::Value::Null,
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Int(i) => serde_json::Value::Number((*i).into()),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Value::String(s) => serde_json::Value::String(s.clone()),
            Value::Array(arr) => serde_json::Value::Array(arr.iter().map(|v| v.to_json()).collect()),
            Value::Object(obj) => {
                let mut m = serde_json::Map::with_capacity(obj.len());
                for (k, v) in obj {
                    m.insert(k.clone(), v.to_json());
                }
                serde_json::Value::Object(m)
            }
            Value::Stream(s) => serde_json::Value::String(format!("_stream:{}", s.index)),
            Value::BStream(s) => serde_json::Value::String(format!("_bstream:{}", s.index)),
        }
    }
}
