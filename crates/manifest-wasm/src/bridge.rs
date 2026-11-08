//! Conversion between JS values and serde types.
//!
//! Inputs are first copied into a [`serde_json::Value`] snapshot and only then
//! deserialized, so serde sees exactly what native Rust sees for the same JSON:
//!
//! - Only own enumerable string keys are read, each getter runs once, and
//!   prototypes are never consulted. Entries whose value is `undefined` are
//!   omitted, like `JSON.stringify`; `undefined` elsewhere reads as `null`.
//! - NaN and ±Infinity are rejected at their exact path instead of becoming
//!   `null`.
//! - At most [`MAX_DEPTH`] containers may nest along any path, which also stops
//!   cyclic inputs. Unknown fields are copied too, so they obey the same limit.
//! - Arrays are read by index up to `length`, like `JSON.stringify`; custom
//!   iterators are not consulted.
//! - BigInt, symbols, and functions are rejected.
//!
//! Decoding then follows `serde_json`, exactly as native Rust decodes JSON
//! text: a struct also accepts a positional array, while a map accepts only an
//! object. Arguments whose shape carries trust, such as the verify policy, are
//! required to be objects before decoding.
//!
//! Outputs are serialized to JSON text and parsed with `JSON.parse`, which
//! creates fresh plain objects whose keys, including `__proto__`, are own data
//! properties.

use js_sys::{Array, JSON, Reflect};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Number, Value};
use wasm_bindgen::prelude::*;

use crate::error::{invalid_input, serialization_failed};

/// Maximum number of objects or arrays along any input path.
const MAX_DEPTH: usize = 64;

/// `Number.MAX_SAFE_INTEGER`; larger magnitudes stay `f64`.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

#[wasm_bindgen]
extern "C" {
    /// `Array.isArray`, which throws for revoked proxies.
    #[wasm_bindgen(catch, js_namespace = Array, js_name = isArray)]
    fn is_array(value: &JsValue) -> Result<bool, JsValue>;

    /// `Object.keys`: own enumerable string keys. Proxy traps may throw.
    #[wasm_bindgen(catch, js_namespace = Object, js_name = keys)]
    fn own_keys(value: &JsValue) -> Result<Array, JsValue>;
}

/// Snapshots and deserializes one argument. Errors use `wasm.invalid_input` at
/// `path`; exceptions thrown by getters or proxies propagate unchanged.
pub(crate) fn decode<T: DeserializeOwned>(value: &JsValue, path: &str) -> Result<T, JsValue> {
    from_value(snapshot(value, path, 0)?, path)
}

/// Deserializes an already snapshotted value.
pub(crate) fn from_value<T: DeserializeOwned>(value: Value, path: &str) -> Result<T, JsValue> {
    serde_json::from_value(value).map_err(|source| invalid_input(path, source))
}

/// Serializes a result as a fresh plain JS value.
pub(crate) fn encode(value: &impl Serialize) -> Result<JsValue, JsValue> {
    let text = serde_json::to_string(value).map_err(serialization_failed)?;
    JSON::parse(&text).map_err(|_| serialization_failed("JSON.parse rejected serde_json output"))
}

/// The fields of an object argument, decoded one at a time so each error
/// carries the field's own path.
pub(crate) struct Fields {
    path: &'static str,
    map: Map<String, Value>,
}

impl Fields {
    /// Snapshots a required object argument. Arrays, functions, and primitives
    /// are rejected before any field is read.
    pub(crate) fn object(value: &JsValue, path: &'static str) -> Result<Self, JsValue> {
        if !value.is_object() || is_array(value)? {
            return Err(invalid_input(path, "expected an object"));
        }
        let Value::Object(map) = snapshot(value, path, 0)? else {
            unreachable!("a non-array object snapshots to a JSON object");
        };
        Ok(Self { path, map })
    }

    /// Like [`Fields::object`], but `null` and `undefined` read as no fields.
    pub(crate) fn optional(value: &JsValue, path: &'static str) -> Result<Self, JsValue> {
        if value.is_null() || value.is_undefined() {
            Ok(Self {
                path,
                map: Map::new(),
            })
        } else {
            Self::object(value, path)
        }
    }

    /// Returns a field without consuming it.
    pub(crate) fn peek(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }

    /// Decodes an optional field; missing and `null` read as `None`.
    pub(crate) fn take<T: DeserializeOwned>(&mut self, key: &str) -> Result<Option<T>, JsValue> {
        match self.map.remove(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => from_value(value, &format!("{}.{key}", self.path)).map(Some),
        }
    }

    /// Decodes a required field.
    pub(crate) fn require<T: DeserializeOwned>(&mut self, key: &str) -> Result<T, JsValue> {
        self.take(key)?
            .ok_or_else(|| invalid_input(&format!("{}.{key}", self.path), "missing required field"))
    }

    /// Deserializes the whole object at the argument's path.
    pub(crate) fn decode<T: DeserializeOwned>(self) -> Result<T, JsValue> {
        from_value(Value::Object(self.map), self.path)
    }
}

fn snapshot(value: &JsValue, path: &str, depth: usize) -> Result<Value, JsValue> {
    if value.is_null() || value.is_undefined() {
        return Ok(Value::Null);
    }
    if let Some(flag) = value.as_bool() {
        return Ok(Value::Bool(flag));
    }
    if let Some(number) = value.as_f64() {
        return number_value(number, path);
    }
    if let Some(text) = value.as_string() {
        return Ok(Value::String(text));
    }
    if !value.is_object() {
        return Err(invalid_input(path, "expected a JSON-compatible value"));
    }
    if depth >= MAX_DEPTH {
        return Err(invalid_input(
            path,
            format_args!("input nesting exceeds {MAX_DEPTH} containers"),
        ));
    }
    if is_array(value)? {
        let length = Reflect::get(value, &JsValue::from_str("length"))?
            .as_f64()
            .unwrap_or_default();
        // An array length is an integer in 0..2^32, so the cast is exact.
        (0..length as u32)
            .map(|index| {
                let entry = Reflect::get_u32(value, index)?;
                snapshot(&entry, &format!("{path}.{index}"), depth + 1)
            })
            .collect::<Result<_, _>>()
            .map(Value::Array)
    } else {
        let mut map = Map::new();
        for key in own_keys(value)?.iter() {
            let entry = Reflect::get(value, &key)?;
            if entry.is_undefined() {
                continue;
            }
            let key = key.as_string().expect("Object.keys returns only strings");
            let entry = snapshot(&entry, &format!("{path}.{key}"), depth + 1)?;
            map.insert(key, entry);
        }
        Ok(Value::Object(map))
    }
}

/// Safe integers become serde integers so integer fields accept them; other
/// finite numbers stay `f64`.
fn number_value(number: f64, path: &str) -> Result<Value, JsValue> {
    if !number.is_finite() {
        return Err(invalid_input(path, "number must be finite"));
    }
    if number.fract() == 0.0 && number.abs() <= MAX_SAFE_INTEGER {
        // Exact: the value is an integer within ±(2^53 - 1).
        return Ok(Value::from(number as i64));
    }
    Ok(Number::from_f64(number)
        .map(Value::Number)
        .expect("finite numbers are representable"))
}
