// Binary serialization format for hirpdag DAGs.
//
// File layout:
//   magic      4 bytes  b"HIRP"
//   version    2 bytes  u16 LE = 1
//   node_count 4 bytes  u32 LE
//   per node:
//     type_tag  4 bytes  u32 LE
//     serial_id 4 bytes  u32 LE
//     field_len 4 bytes  u32 LE
//     field_bytes        (field_len bytes)
//   root_count 4 bytes  u32 LE
//   per root:
//     type_tag  4 bytes  u32 LE
//     serial_id 4 bytes  u32 LE
//
// Field encoding inside field_bytes:
//   u8/i8/bool:  1 byte
//   u16/i16:     2 bytes LE
//   u32/i32/f32: 4 bytes LE
//   u64/i64/f64: 8 bytes LE
//   u128/i128:   16 bytes LE
//   usize/isize: 8 bytes LE  (always 64-bit)
//   str:         u32 LE length + UTF-8 bytes
//   seq_len:     u32 LE
//   option_flag: u8 (0 = None, 1 = Some)
//   variant_idx: u32 LE
//   node_ref:    u32 LE serial_id

use std::io::{Read, Write};

use crate::base::serialize::{
    HirpdagDeserCtx, HirpdagDeserializer, HirpdagFieldDecoder, HirpdagFieldEncoder,
    HirpdagSerializer, SerError,
};

const MAGIC: &[u8; 4] = b"HIRP";
const VERSION: u16 = 1;

// ---------------------------------------------------------------------------
// BinaryFieldEncoder
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct BinaryFieldEncoder {
    buf: Vec<u8>,
}

impl HirpdagFieldEncoder for BinaryFieldEncoder {
    type Output = Vec<u8>;

    fn finish(self) -> Vec<u8> {
        self.buf
    }

    fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn write_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_u128(&mut self, v: u128) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_usize(&mut self, v: usize) {
        self.buf.extend_from_slice(&(v as u64).to_le_bytes());
    }
    fn write_i8(&mut self, v: i8) {
        self.buf.push(v as u8);
    }
    fn write_i16(&mut self, v: i16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_i128(&mut self, v: i128) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_isize(&mut self, v: isize) {
        self.buf.extend_from_slice(&(v as i64).to_le_bytes());
    }
    fn write_f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn write_bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }
    fn write_str(&mut self, v: &str) {
        let bytes = v.as_bytes();
        self.write_u32(bytes.len() as u32);
        self.buf.extend_from_slice(bytes);
    }
    fn write_seq_len(&mut self, len: usize) {
        self.write_u32(len as u32);
    }
    fn write_option_flag(&mut self, is_some: bool) {
        self.buf.push(is_some as u8);
    }
    fn write_variant_idx(&mut self, idx: u32) {
        self.write_u32(idx);
    }
    fn write_node_ref(&mut self, serial_id: u32) {
        self.write_u32(serial_id);
    }
}

// ---------------------------------------------------------------------------
// BinaryFieldDecoder
// ---------------------------------------------------------------------------

pub struct BinaryFieldDecoder<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BinaryFieldDecoder<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        BinaryFieldDecoder { data, pos: 0 }
    }

    fn read_bytes(&mut self, n: usize) -> Result<&[u8], SerError> {
        let end = self.pos + n;
        if end > self.data.len() {
            return Err(SerError::UnexpectedEof);
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
}

impl<'a> HirpdagFieldDecoder for BinaryFieldDecoder<'a> {
    fn read_u8(&mut self) -> Result<u8, SerError> {
        Ok(self.read_bytes(1)?[0])
    }
    fn read_u16(&mut self) -> Result<u16, SerError> {
        Ok(u16::from_le_bytes(self.read_bytes(2)?.try_into().unwrap()))
    }
    fn read_u32(&mut self) -> Result<u32, SerError> {
        Ok(u32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()))
    }
    fn read_u64(&mut self) -> Result<u64, SerError> {
        Ok(u64::from_le_bytes(self.read_bytes(8)?.try_into().unwrap()))
    }
    fn read_u128(&mut self) -> Result<u128, SerError> {
        Ok(u128::from_le_bytes(self.read_bytes(16)?.try_into().unwrap()))
    }
    fn read_usize(&mut self) -> Result<usize, SerError> {
        Ok(u64::from_le_bytes(self.read_bytes(8)?.try_into().unwrap()) as usize)
    }
    fn read_i8(&mut self) -> Result<i8, SerError> {
        Ok(self.read_bytes(1)?[0] as i8)
    }
    fn read_i16(&mut self) -> Result<i16, SerError> {
        Ok(i16::from_le_bytes(self.read_bytes(2)?.try_into().unwrap()))
    }
    fn read_i32(&mut self) -> Result<i32, SerError> {
        Ok(i32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()))
    }
    fn read_i64(&mut self) -> Result<i64, SerError> {
        Ok(i64::from_le_bytes(self.read_bytes(8)?.try_into().unwrap()))
    }
    fn read_i128(&mut self) -> Result<i128, SerError> {
        Ok(i128::from_le_bytes(self.read_bytes(16)?.try_into().unwrap()))
    }
    fn read_isize(&mut self) -> Result<isize, SerError> {
        Ok(i64::from_le_bytes(self.read_bytes(8)?.try_into().unwrap()) as isize)
    }
    fn read_f32(&mut self) -> Result<f32, SerError> {
        Ok(f32::from_le_bytes(self.read_bytes(4)?.try_into().unwrap()))
    }
    fn read_f64(&mut self) -> Result<f64, SerError> {
        Ok(f64::from_le_bytes(self.read_bytes(8)?.try_into().unwrap()))
    }
    fn read_bool(&mut self) -> Result<bool, SerError> {
        Ok(self.read_bytes(1)?[0] != 0)
    }
    fn read_str(&mut self) -> Result<String, SerError> {
        let len = self.read_u32()? as usize;
        let bytes = self.read_bytes(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| SerError::InvalidUtf8)
    }
    fn read_seq_len(&mut self) -> Result<usize, SerError> {
        Ok(self.read_u32()? as usize)
    }
    fn read_option_flag(&mut self) -> Result<bool, SerError> {
        Ok(self.read_bytes(1)?[0] != 0)
    }
    fn read_variant_idx(&mut self) -> Result<u32, SerError> {
        self.read_u32()
    }
    fn read_node_ref(&mut self) -> Result<u32, SerError> {
        self.read_u32()
    }
}

// ---------------------------------------------------------------------------
// write_binary helper
// ---------------------------------------------------------------------------

fn write_u16_le<W: Write>(w: &mut W, v: u16) -> Result<(), SerError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn write_u32_le<W: Write>(w: &mut W, v: u32) -> Result<(), SerError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn read_u16_le<R: Read>(r: &mut R) -> Result<u16, SerError> {
    let mut buf = [0u8; 2];
    r.read_exact(&mut buf).map_err(|_| SerError::UnexpectedEof)?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u32_le<R: Read>(r: &mut R) -> Result<u32, SerError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf).map_err(|_| SerError::UnexpectedEof)?;
    Ok(u32::from_le_bytes(buf))
}

impl HirpdagSerializer<BinaryFieldEncoder> {
    /// Write the serialized DAG to `w` in the hirpdag binary format.
    pub fn write_binary<W: Write>(&self, w: &mut W) -> Result<(), SerError> {
        // magic + version
        w.write_all(MAGIC)?;
        write_u16_le(w, VERSION)?;

        // node records
        write_u32_le(w, self.ctx.records.len() as u32)?;
        for (type_tag, serial_id, field_bytes) in &self.ctx.records {
            write_u32_le(w, *type_tag)?;
            write_u32_le(w, *serial_id)?;
            write_u32_le(w, field_bytes.len() as u32)?;
            w.write_all(field_bytes)?;
        }

        // roots
        write_u32_le(w, self.roots.len() as u32)?;
        for (type_tag, serial_id) in &self.roots {
            write_u32_le(w, *type_tag)?;
            write_u32_le(w, *serial_id)?;
        }

        Ok(())
    }
}

/// Dispatch function type for binary deserialization.
pub type BinaryDispatchFn = fn(
    type_tag: u32,
    serial_id: u32,
    dec: &mut BinaryFieldDecoder,
    ctx: &mut HirpdagDeserCtx,
) -> Result<(), SerError>;

impl HirpdagDeserializer {
    /// Read a hirpdag binary file from `r` and reconstruct all nodes via `dispatch`.
    ///
    /// `dispatch` is the `hirpdag_deser_dispatch_binary` function generated by `#[hirpdag_end]`.
    pub fn from_binary<R: Read>(
        r: &mut R,
        dispatch: BinaryDispatchFn,
    ) -> Result<Self, SerError> {
        // Check magic
        let mut magic = [0u8; 4];
        r.read_exact(&mut magic).map_err(|_| SerError::UnexpectedEof)?;
        if &magic != MAGIC {
            return Err(SerError::InvalidMagic);
        }

        let version = read_u16_le(r)?;
        if version != VERSION {
            return Err(SerError::UnsupportedVersion(version));
        }

        let mut ctx = HirpdagDeserCtx::new();

        let node_count = read_u32_le(r)? as usize;
        for _ in 0..node_count {
            let type_tag = read_u32_le(r)?;
            let serial_id = read_u32_le(r)?;
            let field_len = read_u32_le(r)? as usize;

            let mut field_bytes = vec![0u8; field_len];
            r.read_exact(&mut field_bytes).map_err(|_| SerError::UnexpectedEof)?;

            let mut dec = BinaryFieldDecoder::new(&field_bytes);
            dispatch(type_tag, serial_id, &mut dec, &mut ctx)?;
        }

        let root_count = read_u32_le(r)? as usize;
        let mut roots = Vec::with_capacity(root_count);
        for _ in 0..root_count {
            let type_tag = read_u32_le(r)?;
            let serial_id = read_u32_le(r)?;
            roots.push((type_tag, serial_id));
        }

        Ok(HirpdagDeserializer { ctx, roots })
    }
}
