// Binary serialization format for hirpdag DAGs.
//
// Uses `bincode` (v2, standard config) for field-level encoding/decoding,
// mirroring how the JSON format delegates to `serde_json`.
//
// File layout — all values except the 4-byte magic are bincode-encoded:
//   magic       b"HIRP" (4 raw bytes)
//   version     u16
//   node_count  u64
//   per node (topological order — children before parents):
//     type_tag    u32
//     serial_id   u32
//     field_data  [u8] (length-prefixed bincode encoding of the node's fields)
//   root_count  u64
//   per root:
//     type_tag    u32
//     serial_id   u32
//
// Zero-copy deserialization:
//   `BinaryFieldDecoder<'a>` borrows a `&'a [u8]` and uses
//   `bincode::decode_from_slice`, so reading fixed-size fields does not
//   allocate.  String fields require one copy to produce an owned `String`.
//
//   `HirpdagDeserializer::from_binary_bytes` accepts a `&[u8]` directly;
//   callers can pass a `memmap2::Mmap` deref'd to `&[u8]` for a
//   memory-mapped, allocation-free read path.

use std::io::{Read, Write};

use bincode::config::standard;
use bincode::{decode_from_slice, encode_into_std_write};

use crate::base::serialize::{
    HirpdagDeserCtx, HirpdagDeserializer, HirpdagFieldDecoder, HirpdagFieldEncoder,
    HirpdagSerializer, SerError,
};

const MAGIC: &[u8; 4] = b"HIRP";
const BINCODE_CFG: bincode::config::Configuration = standard();

// ---------------------------------------------------------------------------
// BinaryFieldEncoder — backed by bincode
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct BinaryFieldEncoder {
    buf: Vec<u8>,
}

impl BinaryFieldEncoder {
    #[inline]
    fn enc<T: bincode::Encode>(&mut self, v: T) {
        encode_into_std_write(v, &mut self.buf, BINCODE_CFG)
            .expect("in-memory bincode encode should never fail");
    }
}

impl HirpdagFieldEncoder for BinaryFieldEncoder {
    type Output = Vec<u8>;

    fn finish(self) -> Vec<u8> {
        self.buf
    }

    fn write_u8(&mut self, v: u8)       { self.enc(v); }
    fn write_u16(&mut self, v: u16)     { self.enc(v); }
    fn write_u32(&mut self, v: u32)     { self.enc(v); }
    fn write_u64(&mut self, v: u64)     { self.enc(v); }
    fn write_u128(&mut self, v: u128)   { self.enc(v); }
    fn write_usize(&mut self, v: usize) { self.enc(v as u64); }
    fn write_i8(&mut self, v: i8)       { self.enc(v); }
    fn write_i16(&mut self, v: i16)     { self.enc(v); }
    fn write_i32(&mut self, v: i32)     { self.enc(v); }
    fn write_i64(&mut self, v: i64)     { self.enc(v); }
    fn write_i128(&mut self, v: i128)   { self.enc(v); }
    fn write_isize(&mut self, v: isize) { self.enc(v as i64); }
    fn write_f32(&mut self, v: f32)     { self.enc(v); }
    fn write_f64(&mut self, v: f64)     { self.enc(v); }
    fn write_bool(&mut self, v: bool)   { self.enc(v); }
    fn write_str(&mut self, v: &str)    { self.enc(v); }
    fn write_seq_len(&mut self, len: usize)      { self.enc(len as u64); }
    fn write_option_flag(&mut self, is_some: bool) { self.enc(is_some); }
    fn write_variant_idx(&mut self, idx: u32)    { self.enc(idx); }
    fn write_node_ref(&mut self, serial_id: u32) { self.enc(serial_id); }
}

// ---------------------------------------------------------------------------
// BinaryFieldDecoder — zero-copy slice reader backed by bincode
// ---------------------------------------------------------------------------

/// Reads field values from a borrowed byte slice using `bincode::decode_from_slice`.
///
/// Because the slice is borrowed, fixed-size field reads (integers, booleans,
/// floats) do not allocate.  String fields require a copy into an owned `String`.
/// When `data` originates from a `memmap2::Mmap`, file I/O is also allocation-free.
pub struct BinaryFieldDecoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BinaryFieldDecoder<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BinaryFieldDecoder { data, pos: 0 }
    }

    #[inline]
    fn dec<T: bincode::Decode<()>>(&mut self) -> Result<T, SerError> {
        let (val, consumed) = decode_from_slice(&self.data[self.pos..], BINCODE_CFG)
            .map_err(|e| SerError::Decode(e.to_string()))?;
        self.pos += consumed;
        Ok(val)
    }
}

impl<'a> HirpdagFieldDecoder for BinaryFieldDecoder<'a> {
    fn read_u8(&mut self)    -> Result<u8,    SerError> { self.dec() }
    fn read_u16(&mut self)   -> Result<u16,   SerError> { self.dec() }
    fn read_u32(&mut self)   -> Result<u32,   SerError> { self.dec() }
    fn read_u64(&mut self)   -> Result<u64,   SerError> { self.dec() }
    fn read_u128(&mut self)  -> Result<u128,  SerError> { self.dec() }
    fn read_usize(&mut self) -> Result<usize, SerError> {
        self.dec::<u64>().map(|v| v as usize)
    }
    fn read_i8(&mut self)    -> Result<i8,    SerError> { self.dec() }
    fn read_i16(&mut self)   -> Result<i16,   SerError> { self.dec() }
    fn read_i32(&mut self)   -> Result<i32,   SerError> { self.dec() }
    fn read_i64(&mut self)   -> Result<i64,   SerError> { self.dec() }
    fn read_i128(&mut self)  -> Result<i128,  SerError> { self.dec() }
    fn read_isize(&mut self) -> Result<isize, SerError> {
        self.dec::<i64>().map(|v| v as isize)
    }
    fn read_f32(&mut self)  -> Result<f32,    SerError> { self.dec() }
    fn read_f64(&mut self)  -> Result<f64,    SerError> { self.dec() }
    fn read_bool(&mut self) -> Result<bool,   SerError> { self.dec() }
    fn read_str(&mut self)  -> Result<String, SerError> { self.dec() }
    fn read_seq_len(&mut self)     -> Result<usize, SerError> {
        self.dec::<u64>().map(|v| v as usize)
    }
    fn read_option_flag(&mut self) -> Result<bool,  SerError> { self.dec() }
    fn read_variant_idx(&mut self) -> Result<u32,   SerError> { self.dec() }
    fn read_node_ref(&mut self)    -> Result<u32,   SerError> { self.dec() }
}

// ---------------------------------------------------------------------------
// File-level write / read helpers
// ---------------------------------------------------------------------------

fn write_val<T: bincode::Encode, W: Write>(w: &mut W, v: T) -> Result<(), SerError> {
    encode_into_std_write(v, w, BINCODE_CFG).map_err(|e| SerError::Decode(e.to_string()))?;
    Ok(())
}

fn read_val<'a, T: bincode::Decode<()>>(data: &'a [u8], pos: &mut usize) -> Result<T, SerError> {
    let (val, consumed) = decode_from_slice(&data[*pos..], BINCODE_CFG)
        .map_err(|e| SerError::Decode(e.to_string()))?;
    *pos += consumed;
    Ok(val)
}

// ---------------------------------------------------------------------------
// HirpdagSerializer::write_binary
// ---------------------------------------------------------------------------

impl HirpdagSerializer<BinaryFieldEncoder> {
    /// Write the serialized DAG to `w` in the hirpdag binary format.
    pub fn write_binary<W: Write>(&self, w: &mut W) -> Result<(), SerError> {
        // 4-byte raw magic
        w.write_all(MAGIC)?;
        // version (u16), node_count (u64)
        write_val(w, 1u16)?;
        write_val(w, self.ctx.records.len() as u64)?;
        // node records
        for (type_tag, serial_id, field_bytes) in &self.ctx.records {
            write_val(w, *type_tag)?;
            write_val(w, *serial_id)?;
            // field_data: length-prefixed bincode bytes
            write_val(w, field_bytes.len() as u64)?;
            w.write_all(field_bytes)?;
        }
        // roots
        write_val(w, self.roots.len() as u64)?;
        for (type_tag, serial_id) in &self.roots {
            write_val(w, *type_tag)?;
            write_val(w, *serial_id)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HirpdagDeserializer::from_binary / from_binary_bytes
// ---------------------------------------------------------------------------

/// Dispatch function type for binary deserialization.
pub type BinaryDispatchFn = fn(
    type_tag: u32,
    serial_id: u32,
    dec: &mut BinaryFieldDecoder,
    ctx: &mut HirpdagDeserCtx,
) -> Result<(), SerError>;

impl HirpdagDeserializer {
    /// Read a hirpdag binary file from `r`.
    ///
    /// Reads the entire contents into a `Vec<u8>` then delegates to
    /// [`from_binary_bytes`](Self::from_binary_bytes).
    pub fn from_binary<R: Read>(
        r: &mut R,
        dispatch: BinaryDispatchFn,
    ) -> Result<Self, SerError> {
        let mut data = Vec::new();
        r.read_to_end(&mut data)?;
        Self::from_binary_bytes(&data, dispatch)
    }

    /// Reconstruct all nodes from a raw byte slice.
    ///
    /// This is the **zero-copy** entry point.  Pass the result of
    /// `memmap2::Mmap::deref()` (i.e. `&*mmap`) to avoid any I/O allocation:
    ///
    /// ```no_run
    /// # use std::fs::File;
    /// # use hirpdag::base::serialize::HirpdagDeserializer;
    /// let file = File::open("my.hirp").unwrap();
    /// let mmap = unsafe { memmap2::Mmap::map(&file).unwrap() };
    /// let deser = HirpdagDeserializer::from_binary_bytes(&mmap, hirpdag_deser_dispatch_binary).unwrap();
    /// ```
    ///
    /// Fixed-size field reads (integers, booleans, floats) are zero-copy.
    /// String fields require one allocation per string to produce an owned `String`.
    pub fn from_binary_bytes(
        data: &[u8],
        dispatch: BinaryDispatchFn,
    ) -> Result<Self, SerError> {
        // 4-byte raw magic
        if data.len() < 4 {
            return Err(SerError::UnexpectedEof);
        }
        if &data[..4] != MAGIC {
            return Err(SerError::InvalidMagic);
        }
        let mut pos = 4usize;

        let version: u16 = read_val(data, &mut pos)?;
        if version != 1 {
            return Err(SerError::UnsupportedVersion(version));
        }

        let mut ctx = HirpdagDeserCtx::new();

        let node_count: u64 = read_val(data, &mut pos)?;
        for _ in 0..node_count {
            let type_tag: u32 = read_val(data, &mut pos)?;
            let serial_id: u32 = read_val(data, &mut pos)?;
            let field_len: u64 = read_val(data, &mut pos)?;
            let field_end = pos + field_len as usize;
            if field_end > data.len() {
                return Err(SerError::UnexpectedEof);
            }
            // Zero-copy: pass a sub-slice of the original data directly.
            let mut dec = BinaryFieldDecoder::new(&data[pos..field_end]);
            pos = field_end;
            dispatch(type_tag, serial_id, &mut dec, &mut ctx)?;
        }

        let root_count: u64 = read_val(data, &mut pos)?;
        let mut roots = Vec::with_capacity(root_count as usize);
        for _ in 0..root_count {
            let type_tag: u32 = read_val(data, &mut pos)?;
            let serial_id: u32 = read_val(data, &mut pos)?;
            roots.push((type_tag, serial_id));
        }

        Ok(HirpdagDeserializer { ctx, roots })
    }
}
