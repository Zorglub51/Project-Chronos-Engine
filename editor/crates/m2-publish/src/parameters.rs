//! Per-game parameter contract shared with the Expert editor.
use m2_psb::Value as PsbValue;
use serde_json::Value;
use std::sync::OnceLock;

pub const CATALOG_JSON: &str = include_str!("../../../src/game-parameters.json");
pub const TOP_EXTRAS: &[&str] = &["action", "copy", "desc"];

fn catalog() -> &'static Value {
    static CATALOG: OnceLock<Value> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_str(CATALOG_JSON).expect("parameter catalogue"))
}

/// Validate present overrides only. Absence is meaningful: it inherits the
/// shared title_prof/controller template. Unknown fields are retained.
pub fn validate_game(game: &Value) -> Result<(), String> {
    for field in catalog()["parameters"].as_array().unwrap() {
        if field["ui"] != "expert" {
            continue;
        }
        let path = field["path"].as_str().unwrap();
        let (block, key) = path.split_once('.').unwrap();
        let Some(value) = game.get(block).and_then(|v| v.get(key)) else {
            continue;
        };
        if value.is_null() {
            // Legacy editors stored optional settings as null. Normalization
            // removes these keys before writing or constructing native views.
            if field["optional"] == true {
                continue;
            }
            return Err(format!("{path}: a value is required"));
        }
        let numeric_string = if field["allowNumericString"] == true {
            value
                .as_str()
                .and_then(|s| s.parse::<i32>().ok())
                .map(i64::from)
        } else {
            None
        };
        let integer = value.as_i64().or(numeric_string);
        let valid_type = match field["type"].as_str().unwrap() {
            "integer" => integer.is_some_and(|n| i32::try_from(n).is_ok()),
            "number" => value
                .as_f64()
                .is_some_and(|n| n.is_finite() && n.abs() <= f32::MAX as f64),
            "boolean" => value.is_boolean(),
            "string" => value.as_str().is_some_and(|s| !s.contains('\0')),
            "array" => value.is_array(),
            "object" => value.is_object(),
            _ => false,
        };
        if !valid_type {
            return Err(format!("{path}: expected {}", field["type"]));
        }
        if value.as_object().is_some_and(|object| {
            object
                .keys()
                .any(|key| key == "__chronos_array" || key.contains('\0'))
        }) {
            return Err(format!("{path}: invalid or reserved attribute name"));
        }
        if let Some(number) = value.as_f64().or(integer.map(|n| n as f64)) {
            if field["min"].as_f64().is_some_and(|min| number < min)
                || field["max"].as_f64().is_some_and(|max| number > max)
            {
                return Err(format!("{path}: value outside the supported range"));
            }
        }
        let valid_shape = match field["shape"].as_str().unwrap_or("") {
            "split" => value.as_array().is_some_and(|a| {
                a.len() == 2
                    && a.iter()
                        .all(|n| n.as_u64().is_some_and(|n| (1..=16).contains(&n)))
            }),
            "positiveNumbers" => value.as_array().is_some_and(|a| {
                !a.is_empty()
                    && a.len() <= 32
                    && a.iter()
                        .all(|n| n.as_u64().is_some_and(|n| (1..=60).contains(&n)))
            }),
            "integerMap" => value.as_object().is_some_and(|o| {
                o.values()
                    .all(|n| n.as_i64().is_some_and(|n| i32::try_from(n).is_ok()))
            }),
            "stringMap" => value.as_object().is_some_and(|o| {
                o.values()
                    .all(|v| v.as_str().is_some_and(|s| !s.contains('\0')))
            }),
            "tag" => value.as_str().is_some_and(|s| {
                s.is_empty()
                    || s == "NO"
                    || (s.len() <= 14 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            }),
            _ => true,
        };
        if !valid_shape {
            return Err(format!("{path}: invalid value shape"));
        }
    }
    Ok(())
}

pub fn normalize_game(game: &mut Value) {
    for block in ["rom", "emulator", "rom_alt"] {
        if let Some(object) = game.get_mut(block).and_then(Value::as_object_mut) {
            object.retain(|_, value| !value.is_null());
        }
    }
}

pub(crate) fn to_psb(value: &Value) -> PsbValue {
    match value {
        Value::Null => PsbValue::Null,
        Value::Bool(v) => PsbValue::Bool(*v),
        Value::Number(v) => {
            if let Some(n) = v.as_i64() {
                PsbValue::Int(n)
            } else {
                PsbValue::Float(v.as_f64().unwrap())
            }
        }
        Value::String(v) => PsbValue::String(v.clone()),
        Value::Array(v) => PsbValue::Array(v.iter().map(to_psb).collect()),
        Value::Object(v) => {
            PsbValue::Object(v.iter().map(|(k, v)| (k.clone(), to_psb(v))).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_explicit_zero_false_and_omitted_inheritance() {
        let mut game = json!({"rom":{"overdrive":0,"sram_support":false,"use_analog":null,
            "preamp":6.94,"tg16_option":5,"future":{"list":[1,true]}},"emulator":{"ScreenMode":4}});
        assert!(validate_game(&game).is_ok());
        normalize_game(&mut game);
        assert!(game["rom"].get("use_analog").is_none());
        assert_eq!(game["rom"]["overdrive"], 0);
        assert_eq!(game["rom"]["sram_support"], false);
        assert_eq!(to_psb(&game).to_json(), game);
        assert!(validate_game(&json!({"rom":{"direction":"4"}})).is_ok());
    }

    #[test]
    fn invalid_overrides_fail_before_metadata_is_written() {
        for rom in [
            json!({"tg16_option":16}),
            json!({"tg16pad_type":-1}),
            json!({"overdrive":1.5}),
            json!({"use_analog":"false"}),
            json!({"view_split":[0,1]}),
            json!({"attr":{"x":false}}),
            json!({"rapid_speed_list":[]}),
            json!({"attr":{"__chronos_array":1}}),
            json!({"string":{"x":12}}),
        ] {
            assert!(validate_game(&json!({"rom":rom})).is_err());
        }
    }
}
