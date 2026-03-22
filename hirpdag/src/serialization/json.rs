// JSON serialization format for hirpdag DAGs.
//
// Produces human-readable JSON. The schema is:
//   {
//     "version": 1,
//     "nodes": [
//       { "id": 0, "type_tag": 2345678, "fields": [<field_val>, ...] },
//       ...
//     ],
//     "roots": [
//       { "type_tag": 1234567, "id": 1 },
//       ...
//     ]
//   }
//
// Field encoding:
//   primitives:   JSON number / boolean / string
//   seq_len:      number (precedes array elements; consumed transparently)
//   option_flag:  bool   (precedes optional element; consumed transparently)
//   variant_idx:  { "$v": <idx>, "$d": <next_field> }  (idx consumed by decoder)
//   node_ref:     { "$ref": <serial_id> }

use std::io::{Read, Write};

use serde_json::{json, Value};

use crate::base::serialize::{
    HirpdagDeserCtx, HirpdagDeserializer, HirpdagFieldDecoder, HirpdagFieldEncoder,
    HirpdagSerializer, SerError,
};

const VERSION: u64 = 1;

// ---------------------------------------------------------------------------
// JsonFieldEncoder
// ---------------------------------------------------------------------------

/// Encodes field values as a flat `Vec<serde_json::Value>`.
/// Consumers read them sequentially; each call to a `write_*` method pushes one element.
/// Exception: `write_variant_idx` wraps the next value as `{"$v": idx, "$d": ...}`,
/// which requires the decoder to handle specially.
#[derive(Default)]
pub struct JsonFieldEncoder {
    values: Vec<Value>,
}

impl HirpdagFieldEncoder for JsonFieldEncoder {
    type Output = Vec<Value>;

    fn finish(self) -> Vec<Value> {
        self.values
    }

    fn write_u8(&mut self, v: u8)       { self.values.push(json!(v)); }
    fn write_u16(&mut self, v: u16)     { self.values.push(json!(v)); }
    fn write_u32(&mut self, v: u32)     { self.values.push(json!(v)); }
    fn write_u64(&mut self, v: u64)     { self.values.push(json!(v)); }
    fn write_u128(&mut self, v: u128)   { self.values.push(json!(v.to_string())); }  // JSON can't represent u128 exactly
    fn write_usize(&mut self, v: usize) { self.values.push(json!(v as u64)); }
    fn write_i8(&mut self, v: i8)       { self.values.push(json!(v)); }
    fn write_i16(&mut self, v: i16)     { self.values.push(json!(v)); }
    fn write_i32(&mut self, v: i32)     { self.values.push(json!(v)); }
    fn write_i64(&mut self, v: i64)     { self.values.push(json!(v)); }
    fn write_i128(&mut self, v: i128)   { self.values.push(json!(v.to_string())); }
    fn write_isize(&mut self, v: isize) { self.values.push(json!(v as i64)); }
    fn write_f32(&mut self, v: f32)     { self.values.push(json!(v)); }
    fn write_f64(&mut self, v: f64)     { self.values.push(json!(v)); }
    fn write_bool(&mut self, v: bool)   { self.values.push(json!(v)); }
    fn write_str(&mut self, v: &str)    { self.values.push(json!(v)); }
    fn write_seq_len(&mut self, len: usize) { self.values.push(json!(len as u64)); }
    fn write_option_flag(&mut self, is_some: bool) { self.values.push(json!(is_some)); }
    fn write_variant_idx(&mut self, idx: u32) {
        // Push a sentinel that the decoder will match on.
        // The variant payload is the *next* value(s) pushed after this.
        // Encode as a tagged object so the decoder can identify it.
        self.values.push(json!({ "$v": idx }));
    }
    fn write_node_ref(&mut self, serial_id: u32) {
        self.values.push(json!({ "$ref": serial_id }));
    }
}

// ---------------------------------------------------------------------------
// JsonFieldDecoder
// ---------------------------------------------------------------------------

pub struct JsonFieldDecoder {
    values: Vec<Value>,
    pos: usize,
}

impl JsonFieldDecoder {
    pub fn new(values: Vec<Value>) -> Self {
        JsonFieldDecoder { values, pos: 0 }
    }

    fn next(&mut self) -> Result<Value, SerError> {
        if self.pos >= self.values.len() {
            return Err(SerError::UnexpectedEof);
        }
        let v = self.values[self.pos].take();
        self.pos += 1;
        Ok(v)
    }
}

impl HirpdagFieldDecoder for JsonFieldDecoder {
    fn read_u8(&mut self) -> Result<u8, SerError> {
        self.next()?.as_u64().map(|v| v as u8).ok_or(SerError::TypeMismatch)
    }
    fn read_u16(&mut self) -> Result<u16, SerError> {
        self.next()?.as_u64().map(|v| v as u16).ok_or(SerError::TypeMismatch)
    }
    fn read_u32(&mut self) -> Result<u32, SerError> {
        self.next()?.as_u64().map(|v| v as u32).ok_or(SerError::TypeMismatch)
    }
    fn read_u64(&mut self) -> Result<u64, SerError> {
        self.next()?.as_u64().ok_or(SerError::TypeMismatch)
    }
    fn read_u128(&mut self) -> Result<u128, SerError> {
        let s = match self.next()? {
            Value::String(s) => s,
            _ => return Err(SerError::TypeMismatch),
        };
        s.parse::<u128>().map_err(|_| SerError::TypeMismatch)
    }
    fn read_usize(&mut self) -> Result<usize, SerError> {
        self.next()?.as_u64().map(|v| v as usize).ok_or(SerError::TypeMismatch)
    }
    fn read_i8(&mut self) -> Result<i8, SerError> {
        self.next()?.as_i64().map(|v| v as i8).ok_or(SerError::TypeMismatch)
    }
    fn read_i16(&mut self) -> Result<i16, SerError> {
        self.next()?.as_i64().map(|v| v as i16).ok_or(SerError::TypeMismatch)
    }
    fn read_i32(&mut self) -> Result<i32, SerError> {
        self.next()?.as_i64().map(|v| v as i32).ok_or(SerError::TypeMismatch)
    }
    fn read_i64(&mut self) -> Result<i64, SerError> {
        self.next()?.as_i64().ok_or(SerError::TypeMismatch)
    }
    fn read_i128(&mut self) -> Result<i128, SerError> {
        let s = match self.next()? {
            Value::String(s) => s,
            _ => return Err(SerError::TypeMismatch),
        };
        s.parse::<i128>().map_err(|_| SerError::TypeMismatch)
    }
    fn read_isize(&mut self) -> Result<isize, SerError> {
        self.next()?.as_i64().map(|v| v as isize).ok_or(SerError::TypeMismatch)
    }
    fn read_f32(&mut self) -> Result<f32, SerError> {
        self.next()?.as_f64().map(|v| v as f32).ok_or(SerError::TypeMismatch)
    }
    fn read_f64(&mut self) -> Result<f64, SerError> {
        self.next()?.as_f64().ok_or(SerError::TypeMismatch)
    }
    fn read_bool(&mut self) -> Result<bool, SerError> {
        self.next()?.as_bool().ok_or(SerError::TypeMismatch)
    }
    fn read_str(&mut self) -> Result<String, SerError> {
        match self.next()? {
            Value::String(s) => Ok(s),
            _ => Err(SerError::TypeMismatch),
        }
    }
    fn read_seq_len(&mut self) -> Result<usize, SerError> {
        self.next()?.as_u64().map(|v| v as usize).ok_or(SerError::TypeMismatch)
    }
    fn read_option_flag(&mut self) -> Result<bool, SerError> {
        self.next()?.as_bool().ok_or(SerError::TypeMismatch)
    }
    fn read_variant_idx(&mut self) -> Result<u32, SerError> {
        let v = self.next()?;
        v.get("$v")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32)
            .ok_or(SerError::TypeMismatch)
    }
    fn read_node_ref(&mut self) -> Result<u32, SerError> {
        let v = self.next()?;
        v.get("$ref")
            .and_then(|x| x.as_u64())
            .map(|x| x as u32)
            .ok_or(SerError::TypeMismatch)
    }
}

// ---------------------------------------------------------------------------
// write_json / from_json
// ---------------------------------------------------------------------------

impl HirpdagSerializer<JsonFieldEncoder> {
    /// Write the serialized DAG to `w` as JSON.
    pub fn write_json<W: Write>(&self, w: &mut W) -> Result<(), SerError> {
        let nodes: Vec<Value> = self
            .ctx
            .records
            .iter()
            .map(|(type_tag, serial_id, fields)| {
                json!({
                    "id": serial_id,
                    "type_tag": type_tag,
                    "fields": fields,
                })
            })
            .collect();

        let roots: Vec<Value> = self
            .roots
            .iter()
            .map(|(type_tag, serial_id)| json!({ "type_tag": type_tag, "id": serial_id }))
            .collect();

        let doc = json!({
            "version": VERSION,
            "nodes": nodes,
            "roots": roots,
        });

        serde_json::to_writer(w, &doc).map_err(|e| SerError::Json(e.to_string()))?;
        Ok(())
    }
}

/// Dispatch function type for JSON deserialization.
pub type JsonDispatchFn = fn(
    type_tag: u32,
    serial_id: u32,
    dec: &mut JsonFieldDecoder,
    ctx: &mut HirpdagDeserCtx,
) -> Result<(), SerError>;

impl HirpdagDeserializer {
    /// Read a hirpdag JSON file from `r` and reconstruct all nodes via `dispatch`.
    ///
    /// `dispatch` is the `hirpdag_deser_dispatch_json` function generated by `#[hirpdag_end]`.
    pub fn from_json<R: Read>(r: &mut R, dispatch: JsonDispatchFn) -> Result<Self, SerError> {
        let doc: Value =
            serde_json::from_reader(r).map_err(|e| SerError::Json(e.to_string()))?;

        let version = doc["version"].as_u64().ok_or(SerError::TypeMismatch)?;
        if version != VERSION {
            return Err(SerError::UnsupportedVersion(version as u16));
        }

        let mut ctx = HirpdagDeserCtx::new();

        let nodes = doc["nodes"].as_array().ok_or(SerError::TypeMismatch)?;
        for node in nodes {
            let type_tag = node["type_tag"].as_u64().ok_or(SerError::TypeMismatch)? as u32;
            let serial_id = node["id"].as_u64().ok_or(SerError::TypeMismatch)? as u32;
            let fields = node["fields"].as_array().ok_or(SerError::TypeMismatch)?.clone();
            let mut dec = JsonFieldDecoder::new(fields);
            dispatch(type_tag, serial_id, &mut dec, &mut ctx)?;
        }

        let raw_roots = doc["roots"].as_array().ok_or(SerError::TypeMismatch)?;
        let mut roots = Vec::with_capacity(raw_roots.len());
        for r in raw_roots {
            let type_tag = r["type_tag"].as_u64().ok_or(SerError::TypeMismatch)? as u32;
            let serial_id = r["id"].as_u64().ok_or(SerError::TypeMismatch)? as u32;
            roots.push((type_tag, serial_id));
        }

        Ok(HirpdagDeserializer { ctx, roots })
    }
}
