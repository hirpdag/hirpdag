// DAG-aware serialization / deserialization infrastructure.
//
// Key types:
//   HirpdagFieldEncoder  – trait for writing individual field values to an output format
//   HirpdagFieldDecoder  – trait for reading individual field values from an input format
//   HirpdagSerField      – implemented by every serializable type (primitives, enums, node refs)
//   HirpdagDeserField    – implemented by every deserializable type
//   HirpdagSerNode       – implemented by hirpdag struct wrappers; handles DAG dedup
//   HirpdagDeserNode     – implemented by hirpdag struct wrappers; reconstructs via hashcons
//   HirpdagSerCtx        – tracks creation_id → serial_id mapping, accumulates records
//   HirpdagDeserCtx      – maps serial_id → type-erased reconstructed node
//   HirpdagSerializer    – user-facing builder; add roots then write to a format
//   HirpdagDeserializer  – user-facing; read from a format and retrieve roots by index

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum SerError {
    Io(std::io::Error),
    UnknownTypeTag(u32),
    UnknownVariant(u32),
    MissingNode(u32),
    TypeMismatch,
    InvalidMagic,
    UnsupportedVersion(u16),
    InvalidUtf8,
    UnexpectedEof,
    Json(String),
}

impl std::fmt::Display for SerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl From<std::io::Error> for SerError {
    fn from(e: std::io::Error) -> Self {
        SerError::Io(e)
    }
}

// ---------------------------------------------------------------------------
// Compile-time FNV-1a hash (used for type tags)
// ---------------------------------------------------------------------------

pub const fn hirpdag_fnv1a(s: &[u8]) -> u32 {
    let mut hash = 2166136261u32;
    let mut i = 0;
    while i < s.len() {
        hash ^= s[i] as u32;
        hash = hash.wrapping_mul(16777619);
        i += 1;
    }
    hash
}

// ---------------------------------------------------------------------------
// HirpdagFieldEncoder
// ---------------------------------------------------------------------------

/// Abstracts the output format for field-level serialization.
///
/// Each hirpdag node creates a fresh `FE::default()`, writes all its fields into it,
/// then calls `finish()` to obtain the encoded output for that node's record.
pub trait HirpdagFieldEncoder: Default {
    /// The encoded representation produced for one node's fields.
    type Output;

    fn finish(self) -> Self::Output;

    fn write_u8(&mut self, v: u8);
    fn write_u16(&mut self, v: u16);
    fn write_u32(&mut self, v: u32);
    fn write_u64(&mut self, v: u64);
    fn write_u128(&mut self, v: u128);
    fn write_usize(&mut self, v: usize);
    fn write_i8(&mut self, v: i8);
    fn write_i16(&mut self, v: i16);
    fn write_i32(&mut self, v: i32);
    fn write_i64(&mut self, v: i64);
    fn write_i128(&mut self, v: i128);
    fn write_isize(&mut self, v: isize);
    fn write_f32(&mut self, v: f32);
    fn write_f64(&mut self, v: f64);
    fn write_bool(&mut self, v: bool);
    fn write_str(&mut self, v: &str);
    /// Length prefix for `Vec<T>`.
    fn write_seq_len(&mut self, len: usize);
    /// Presence byte for `Option<T>` (false = None, true = Some).
    fn write_option_flag(&mut self, is_some: bool);
    /// Discriminant index for enum variants.
    fn write_variant_idx(&mut self, idx: u32);
    /// Reference to another already-serialized node (by serial_id).
    fn write_node_ref(&mut self, serial_id: u32);
}

// ---------------------------------------------------------------------------
// HirpdagFieldDecoder
// ---------------------------------------------------------------------------

/// Abstracts the input format for field-level deserialization.
pub trait HirpdagFieldDecoder {
    fn read_u8(&mut self)    -> Result<u8,    SerError>;
    fn read_u16(&mut self)   -> Result<u16,   SerError>;
    fn read_u32(&mut self)   -> Result<u32,   SerError>;
    fn read_u64(&mut self)   -> Result<u64,   SerError>;
    fn read_u128(&mut self)  -> Result<u128,  SerError>;
    fn read_usize(&mut self) -> Result<usize, SerError>;
    fn read_i8(&mut self)    -> Result<i8,    SerError>;
    fn read_i16(&mut self)   -> Result<i16,   SerError>;
    fn read_i32(&mut self)   -> Result<i32,   SerError>;
    fn read_i64(&mut self)   -> Result<i64,   SerError>;
    fn read_i128(&mut self)  -> Result<i128,  SerError>;
    fn read_isize(&mut self) -> Result<isize, SerError>;
    fn read_f32(&mut self)   -> Result<f32,   SerError>;
    fn read_f64(&mut self)   -> Result<f64,   SerError>;
    fn read_bool(&mut self)  -> Result<bool,  SerError>;
    fn read_str(&mut self)   -> Result<String, SerError>;
    fn read_seq_len(&mut self)     -> Result<usize, SerError>;
    fn read_option_flag(&mut self) -> Result<bool,  SerError>;
    fn read_variant_idx(&mut self) -> Result<u32,   SerError>;
    fn read_node_ref(&mut self)    -> Result<u32,   SerError>;
}

// ---------------------------------------------------------------------------
// Field-level traits
// ---------------------------------------------------------------------------

/// Serialize a value as a field (writes directly into the encoder).
/// For hirpdag node wrappers this writes a `NodeRef`; for other types it writes inline data.
pub trait HirpdagSerField<FE: HirpdagFieldEncoder> {
    fn hirpdag_ser_field(&self, enc: &mut FE, ctx: &mut HirpdagSerCtx<FE>);
}

/// Deserialize a value from a field decoder.
pub trait HirpdagDeserField<FD: HirpdagFieldDecoder>: Sized {
    fn hirpdag_deser_field(dec: &mut FD, ctx: &HirpdagDeserCtx) -> Result<Self, SerError>;
}

// ---------------------------------------------------------------------------
// Node-level traits (hirpdag struct wrappers only)
// ---------------------------------------------------------------------------

/// DAG-aware node serialization. Implemented on the public wrapper (e.g. `Expr`).
pub trait HirpdagSerNode<FE: HirpdagFieldEncoder>: HirpdagSerField<FE> {
    /// Compile-time type tag (FNV-1a hash of the type name).
    const TYPE_TAG: u32;
    /// Serialize this node and all transitive dependencies into `ctx`.
    /// Returns the serial_id assigned to this node (idempotent: same node returns the same id).
    fn hirpdag_ser_node(&self, ctx: &mut HirpdagSerCtx<FE>) -> u32;
}

/// Node deserialization. Implemented on the public wrapper.
pub trait HirpdagDeserNode<FD: HirpdagFieldDecoder>: Sized {
    fn hirpdag_deser_node(
        serial_id: u32,
        dec: &mut FD,
        ctx: &mut HirpdagDeserCtx,
    ) -> Result<Self, SerError>;
}

// ---------------------------------------------------------------------------
// Serialization context
// ---------------------------------------------------------------------------

pub struct HirpdagSerCtx<FE: HirpdagFieldEncoder> {
    /// Maps creation_id → assigned serial_id
    pub id_map: HashMap<u64, u32>,
    pub next_id: u32,
    /// Completed node records in topological order (dependencies before parents).
    pub records: Vec<(u32, u32, FE::Output)>,  // (type_tag, serial_id, field_output)
}

impl<FE: HirpdagFieldEncoder> HirpdagSerCtx<FE> {
    pub fn new() -> Self {
        HirpdagSerCtx {
            id_map: HashMap::new(),
            next_id: 0,
            records: Vec::new(),
        }
    }
}

impl<FE: HirpdagFieldEncoder> Default for HirpdagSerCtx<FE> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Deserialization context
// ---------------------------------------------------------------------------

pub struct HirpdagDeserCtx {
    nodes: HashMap<u32, Box<dyn std::any::Any + Send + Sync>>,
}

impl HirpdagDeserCtx {
    pub fn new() -> Self {
        HirpdagDeserCtx {
            nodes: HashMap::new(),
        }
    }

    pub fn get_node<T: Clone + std::any::Any + Send + Sync>(&self, serial_id: u32) -> Option<T> {
        self.nodes.get(&serial_id)?.downcast_ref::<T>().cloned()
    }

    pub fn insert_node<T: std::any::Any + Send + Sync>(&mut self, serial_id: u32, node: T) {
        self.nodes.insert(serial_id, Box::new(node));
    }
}

impl Default for HirpdagDeserCtx {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Public serializer / deserializer
// ---------------------------------------------------------------------------

pub struct HirpdagSerializer<FE: HirpdagFieldEncoder> {
    pub ctx: HirpdagSerCtx<FE>,
    pub roots: Vec<(u32, u32)>,  // (type_tag, serial_id)
}

impl<FE: HirpdagFieldEncoder> HirpdagSerializer<FE> {
    pub fn new() -> Self {
        HirpdagSerializer {
            ctx: HirpdagSerCtx::new(),
            roots: Vec::new(),
        }
    }

    pub fn add_root<T: HirpdagSerNode<FE>>(&mut self, root: &T) {
        let sid = root.hirpdag_ser_node(&mut self.ctx);
        self.roots.push((T::TYPE_TAG, sid));
    }
}

impl<FE: HirpdagFieldEncoder> Default for HirpdagSerializer<FE> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HirpdagDeserializer {
    pub ctx: HirpdagDeserCtx,
    pub roots: Vec<(u32, u32)>,  // (type_tag, serial_id)
}

impl HirpdagDeserializer {
    pub fn root_count(&self) -> usize {
        self.roots.len()
    }

    pub fn get_root<T: Clone + std::any::Any + Send + Sync>(&self, idx: usize) -> Option<T> {
        let (_, sid) = self.roots.get(idx)?;
        self.ctx.get_node::<T>(*sid)
    }

    pub fn root_type_tag(&self, idx: usize) -> Option<u32> {
        self.roots.get(idx).map(|(tag, _)| *tag)
    }
}

// ---------------------------------------------------------------------------
// Blanket impls for primitive types
// ---------------------------------------------------------------------------

macro_rules! impl_prim {
    ($t:ty, $write:ident, $read:ident) => {
        impl<FE: HirpdagFieldEncoder> HirpdagSerField<FE> for $t {
            fn hirpdag_ser_field(&self, enc: &mut FE, _ctx: &mut HirpdagSerCtx<FE>) {
                enc.$write(*self);
            }
        }
        impl<FD: HirpdagFieldDecoder> HirpdagDeserField<FD> for $t {
            fn hirpdag_deser_field(
                dec: &mut FD,
                _ctx: &HirpdagDeserCtx,
            ) -> Result<Self, SerError> {
                dec.$read()
            }
        }
    };
}

impl_prim!(u8,    write_u8,    read_u8);
impl_prim!(u16,   write_u16,   read_u16);
impl_prim!(u32,   write_u32,   read_u32);
impl_prim!(u64,   write_u64,   read_u64);
impl_prim!(u128,  write_u128,  read_u128);
impl_prim!(usize, write_usize, read_usize);
impl_prim!(i8,    write_i8,    read_i8);
impl_prim!(i16,   write_i16,   read_i16);
impl_prim!(i32,   write_i32,   read_i32);
impl_prim!(i64,   write_i64,   read_i64);
impl_prim!(i128,  write_i128,  read_i128);
impl_prim!(isize, write_isize, read_isize);
impl_prim!(f32,   write_f32,   read_f32);
impl_prim!(f64,   write_f64,   read_f64);
impl_prim!(bool,  write_bool,  read_bool);

impl<FE: HirpdagFieldEncoder> HirpdagSerField<FE> for String {
    fn hirpdag_ser_field(&self, enc: &mut FE, _ctx: &mut HirpdagSerCtx<FE>) {
        enc.write_str(self.as_str());
    }
}

impl<FD: HirpdagFieldDecoder> HirpdagDeserField<FD> for String {
    fn hirpdag_deser_field(dec: &mut FD, _ctx: &HirpdagDeserCtx) -> Result<Self, SerError> {
        dec.read_str()
    }
}

// ---------------------------------------------------------------------------
// Blanket impls for Vec<T> and Option<T>
// ---------------------------------------------------------------------------

impl<FE: HirpdagFieldEncoder, T: HirpdagSerField<FE>> HirpdagSerField<FE> for Vec<T> {
    fn hirpdag_ser_field(&self, enc: &mut FE, ctx: &mut HirpdagSerCtx<FE>) {
        enc.write_seq_len(self.len());
        for item in self {
            item.hirpdag_ser_field(enc, ctx);
        }
    }
}

impl<FD: HirpdagFieldDecoder, T: HirpdagDeserField<FD>> HirpdagDeserField<FD> for Vec<T> {
    fn hirpdag_deser_field(dec: &mut FD, ctx: &HirpdagDeserCtx) -> Result<Self, SerError> {
        let len = dec.read_seq_len()?;
        let mut v = Vec::with_capacity(len);
        for _ in 0..len {
            v.push(T::hirpdag_deser_field(dec, ctx)?);
        }
        Ok(v)
    }
}

impl<FE: HirpdagFieldEncoder, T: HirpdagSerField<FE>> HirpdagSerField<FE> for Option<T> {
    fn hirpdag_ser_field(&self, enc: &mut FE, ctx: &mut HirpdagSerCtx<FE>) {
        enc.write_option_flag(self.is_some());
        if let Some(v) = self {
            v.hirpdag_ser_field(enc, ctx);
        }
    }
}

impl<FD: HirpdagFieldDecoder, T: HirpdagDeserField<FD>> HirpdagDeserField<FD> for Option<T> {
    fn hirpdag_deser_field(dec: &mut FD, ctx: &HirpdagDeserCtx) -> Result<Self, SerError> {
        let is_some = dec.read_option_flag()?;
        if is_some {
            Ok(Some(T::hirpdag_deser_field(dec, ctx)?))
        } else {
            Ok(None)
        }
    }
}
