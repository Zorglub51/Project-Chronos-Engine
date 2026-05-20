// PSB binary reader. Mirrors the layout decoded by `PsbReader` in mzstool.py.

use crate::value::{Stream, Value};
use crate::Error;
use indexmap::IndexMap;

const MAGIC: &[u8; 4] = b"PSB\0";

// PSB token type IDs (mzstool.py PsbType).
const T_NULL: u8 = 1;
const T_FALSE: u8 = 2;
const T_TRUE: u8 = 3;
const T_INT_0: u8 = 4;
const T_INT_32: u8 = 8;
const T_LONG_40: u8 = 9;
const T_LONG_64: u8 = 12;
const T_UINT_8: u8 = 13;
const T_UINT_32: u8 = 16;
const T_KEY_8: u8 = 17;
const T_KEY_32: u8 = 20;
const T_STRING_8: u8 = 21;
const T_STRING_32: u8 = 24;
const T_STREAM_8: u8 = 25;
const T_STREAM_32: u8 = 28;
const T_FLOAT_0: u8 = 29;
const T_FLOAT: u8 = 30;
const T_DOUBLE: u8 = 31;
const T_ARRAY: u8 = 32;
const T_OBJECT: u8 = 33;
const T_BSTREAM_8: u8 = 34;
const T_BSTREAM_32: u8 = 37;

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    pub version: u16,
    pub flags: u16,
    keys: Vec<String>,
    strings: Vec<String>,
    streams: Vec<Vec<u8>>,
    bstreams: Vec<Vec<u8>>,
}

/// Convenience: parse a PSB file into a Value tree.
pub fn read(data: &[u8]) -> Result<Value, Error> {
    Reader::new(data).parse()
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            version: 0,
            flags: 0,
            keys: Vec::new(),
            strings: Vec::new(),
            streams: Vec::new(),
            bstreams: Vec::new(),
        }
    }

    pub fn parse(&mut self) -> Result<Value, Error> {
        let magic = self.read_bytes(4)?;
        if magic != MAGIC {
            return Err(Error::BadMagic);
        }
        self.version = self.read_u16()?;
        self.flags = self.read_u16()?;

        let keys_offsets_offset = self.read_u32()? as usize;
        let keys_blob_offset = self.read_u32()? as usize;
        let strings_offsets_offset = self.read_u32()? as usize;
        let strings_blob_offset = self.read_u32()? as usize;
        let streams_offsets_offset = self.read_u32()? as usize;
        let streams_sizes_offset = self.read_u32()? as usize;
        let streams_blob_offset = self.read_u32()? as usize;
        let root_offset = self.read_u32()? as usize;

        if self.version >= 3 {
            let _checksum = self.read_u32()?;
        }

        let mut bstreams_offsets_offset = 0usize;
        let mut bstreams_sizes_offset = 0usize;
        let mut bstreams_blob_offset = 0usize;
        if self.version >= 4 {
            bstreams_offsets_offset = self.read_u32()? as usize;
            bstreams_sizes_offset = self.read_u32()? as usize;
            bstreams_blob_offset = self.read_u32()? as usize;
        }

        // Key names — v1 uses a flat offset array, v2+ uses a parent-pointer trie
        if self.version == 1 {
            self.pos = keys_offsets_offset;
            self.keys = self.read_key_names_v1(keys_blob_offset)?;
        } else {
            self.pos = keys_blob_offset;
            self.keys = self.read_key_names_v2()?;
        }

        // Strings
        self.pos = strings_offsets_offset;
        self.strings = self.read_strings(strings_blob_offset)?;

        // Streams
        self.streams = self.read_streams(
            streams_offsets_offset,
            streams_sizes_offset,
            streams_blob_offset,
        )?;

        // BStreams (v4+)
        if self.version >= 4 {
            self.bstreams = self.read_streams(
                bstreams_offsets_offset,
                bstreams_sizes_offset,
                bstreams_blob_offset,
            )?;
        }

        // Root
        self.pos = root_offset;
        self.read_token()
    }

    // ---- low-level binary reads ----

    fn read_u8(&mut self) -> Result<u8, Error> {
        let b = *self.data.get(self.pos).ok_or(Error::Truncated(self.pos))?;
        self.pos += 1;
        Ok(b)
    }

    fn read_u16(&mut self) -> Result<u16, Error> {
        let bs = self.peek_bytes(2)?;
        let v = u16::from_le_bytes([bs[0], bs[1]]);
        self.pos += 2;
        Ok(v)
    }

    fn read_u32(&mut self) -> Result<u32, Error> {
        let bs = self.peek_bytes(4)?;
        let v = u32::from_le_bytes([bs[0], bs[1], bs[2], bs[3]]);
        self.pos += 4;
        Ok(v)
    }

    fn peek_bytes(&self, n: usize) -> Result<&[u8], Error> {
        self.data
            .get(self.pos..self.pos + n)
            .ok_or(Error::Truncated(self.pos))
    }

    fn read_bytes(&mut self, n: usize) -> Result<&[u8], Error> {
        let s = self.data.get(self.pos..self.pos + n).ok_or(Error::Truncated(self.pos))?;
        self.pos += n;
        Ok(s)
    }

    fn read_n_bytes_int(&mut self, n: usize, signed: bool) -> Result<i64, Error> {
        if n == 0 {
            return Ok(0);
        }
        let bytes = self.read_bytes(n)?;
        let mut acc: u64 = 0;
        for (i, b) in bytes.iter().enumerate() {
            acc |= (*b as u64) << (i * 8);
        }
        if signed && n < 8 {
            // Sign-extend from n-byte signed integer
            let sign_bit = 1u64 << (n * 8 - 1);
            if acc & sign_bit != 0 {
                let mask = !((1u64 << (n * 8)) - 1);
                acc |= mask;
            }
        }
        Ok(acc as i64)
    }

    // ---- variable-width unsigned int / array ----

    fn read_uint_by_type(&mut self, ty: u8) -> Result<u64, Error> {
        match ty {
            0 => Ok(0),
            T_UINT_8 => self.read_u8().map(|v| v as u64),
            T_UINT_16 => self.read_u16().map(|v| v as u64),
            T_UINT_24 => Ok(self.read_n_bytes_int(3, false)? as u64),
            T_UINT_32 => self.read_u32().map(|v| v as u64),
            other => Err(Error::InvalidUintType(other)),
        }
    }

    fn read_uint_array(&mut self) -> Result<Vec<u64>, Error> {
        let count_type = self.read_u8()?;
        let count = self.read_uint_by_type(count_type)? as usize;
        let elem_type = self.read_u8()?;
        if elem_type == 0 {
            return Ok(vec![0; count]);
        }
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(self.read_uint_by_type(elem_type)?);
        }
        Ok(out)
    }

    // ---- key/string/stream tables ----

    fn read_key_names_v1(&mut self, blob_offset: usize) -> Result<Vec<String>, Error> {
        let offsets = self.read_uint_array()?;
        let mut names = Vec::with_capacity(offsets.len());
        for off in offsets {
            let pos = blob_offset + off as usize;
            names.push(self.read_cstring_at(pos));
        }
        Ok(names)
    }

    /// Decode v2+ trie-encoded key names. Three parallel arrays:
    ///   - value_offsets: mapping from a node's index to the base of its child range
    ///   - tree:          parent pointer for each node (0 = root sentinel)
    ///   - tails:         one entry per name, pointing at a leaf node
    /// To recover a name, walk up parent pointers from `tree[tails[i]]` until we
    /// hit the root, recording the byte at each step (= node_index - parent_value_offset).
    fn read_key_names_v2(&mut self) -> Result<Vec<String>, Error> {
        let value_offsets = self.read_uint_array()?;
        let tree = self.read_uint_array()?;
        let tails = self.read_uint_array()?;

        let get = |arr: &[u64], i: u64| -> u64 {
            arr.get(i as usize).copied().unwrap_or(0)
        };

        let mut names = Vec::with_capacity(tails.len());
        for &tail in &tails {
            let mut bytes: Vec<u8> = Vec::new();
            let mut current = get(&tree, tail);
            while current != 0 {
                let parent = get(&tree, current);
                let parent_off = get(&value_offsets, parent);
                let ch = current.wrapping_sub(parent_off) as u8;
                bytes.push(ch);
                current = parent;
            }
            bytes.reverse();
            names.push(String::from_utf8_lossy(&bytes).into_owned());
        }
        Ok(names)
    }

    fn read_strings(&mut self, blob_offset: usize) -> Result<Vec<String>, Error> {
        let offsets = self.read_uint_array()?;
        let mut out = Vec::with_capacity(offsets.len());
        for off in offsets {
            out.push(self.read_cstring_at(blob_offset + off as usize));
        }
        Ok(out)
    }

    fn read_cstring_at(&self, start: usize) -> String {
        let mut end = start;
        while end < self.data.len() && self.data[end] != 0 {
            end += 1;
        }
        String::from_utf8_lossy(&self.data[start..end]).into_owned()
    }

    fn read_streams(
        &mut self,
        offsets_offset: usize,
        sizes_offset: usize,
        data_offset: usize,
    ) -> Result<Vec<Vec<u8>>, Error> {
        self.pos = offsets_offset;
        let offsets = self.read_uint_array()?;
        self.pos = sizes_offset;
        let sizes = self.read_uint_array()?;

        let mut streams = Vec::with_capacity(offsets.len());
        for (off, size) in offsets.into_iter().zip(sizes.into_iter()) {
            let start = data_offset + off as usize;
            let end = start + size as usize;
            if end > self.data.len() {
                return Err(Error::Truncated(end));
            }
            streams.push(self.data[start..end].to_vec());
        }
        Ok(streams)
    }

    // ---- token tree ----

    fn read_packed_int(&mut self, ty: u8) -> Result<u64, Error> {
        // Width is encoded as ty - base, where base is the start of the type
        // group (KEY_8=17, STRING_8=21, STREAM_8=25, BSTREAM_8=34, UINT_8=13).
        let width = match ty {
            T_KEY_8..=T_KEY_32 => ty - T_KEY_8 + 1,
            T_STRING_8..=T_STRING_32 => ty - T_STRING_8 + 1,
            T_STREAM_8..=T_STREAM_32 => ty - T_STREAM_8 + 1,
            T_BSTREAM_8..=T_BSTREAM_32 => ty - T_BSTREAM_8 + 1,
            T_UINT_8..=T_UINT_32 => ty - T_UINT_8 + 1,
            _ => return Err(Error::InvalidUintType(ty)),
        };
        match width {
            1 => self.read_u8().map(|v| v as u64),
            2 => self.read_u16().map(|v| v as u64),
            3 => Ok(self.read_n_bytes_int(3, false)? as u64),
            4 => self.read_u32().map(|v| v as u64),
            _ => unreachable!(),
        }
    }

    fn read_token(&mut self) -> Result<Value, Error> {
        let token_pos = self.pos;
        let ty = self.read_u8()?;
        match ty {
            T_NULL => Ok(Value::Null),
            T_FALSE => Ok(Value::Bool(false)),
            T_TRUE => Ok(Value::Bool(true)),

            // INT_0..INT_32 → 0..4 byte signed
            T_INT_0..=T_INT_32 => {
                let n = (ty - T_INT_0) as usize;
                Ok(Value::Int(self.read_n_bytes_int(n, true)?))
            }
            // LONG_40..LONG_64 → 5..8 byte signed
            T_LONG_40..=T_LONG_64 => {
                let n = (ty - T_LONG_40) as usize + 5;
                Ok(Value::Int(self.read_n_bytes_int(n, true)?))
            }

            // KEY_8..KEY_32 → key index
            T_KEY_8..=T_KEY_32 => {
                let idx = self.read_packed_int(ty)? as usize;
                Ok(Value::String(
                    self.keys.get(idx).cloned().unwrap_or_else(|| format!("_key:{}", idx)),
                ))
            }
            // STRING_8..STRING_32 → string index
            T_STRING_8..=T_STRING_32 => {
                let idx = self.read_packed_int(ty)? as usize;
                Ok(Value::String(
                    self.strings.get(idx).cloned().unwrap_or_default(),
                ))
            }
            // STREAM_8..STREAM_32 → stream index
            T_STREAM_8..=T_STREAM_32 => {
                let idx = self.read_packed_int(ty)? as usize;
                let data = self.streams.get(idx).cloned().unwrap_or_default();
                Ok(Value::Stream(Stream { index: idx as u32, data }))
            }
            // BSTREAM_8..BSTREAM_32 (v4+)
            T_BSTREAM_8..=T_BSTREAM_32 => {
                let idx = self.read_packed_int(ty)? as usize;
                let data = self.bstreams.get(idx).cloned().unwrap_or_default();
                Ok(Value::BStream(Stream { index: idx as u32, data }))
            }

            T_FLOAT_0 => Ok(Value::Float(0.0)),
            T_FLOAT => {
                let bs = self.read_bytes(4)?;
                Ok(Value::Float(f32::from_le_bytes([bs[0], bs[1], bs[2], bs[3]]) as f64))
            }
            T_DOUBLE => {
                let bs = self.read_bytes(8)?;
                Ok(Value::Float(f64::from_le_bytes([
                    bs[0], bs[1], bs[2], bs[3], bs[4], bs[5], bs[6], bs[7],
                ])))
            }

            T_ARRAY => self.read_array(),
            T_OBJECT => self.read_object(),

            other => Err(Error::UnknownType { ty: other, pos: token_pos }),
        }
    }

    fn read_array(&mut self) -> Result<Value, Error> {
        let offsets = self.read_uint_array()?;
        let base = self.pos;
        let mut out = Vec::with_capacity(offsets.len());
        for off in offsets {
            self.pos = base + off as usize;
            out.push(self.read_token()?);
        }
        Ok(Value::Array(out))
    }

    fn read_object(&mut self) -> Result<Value, Error> {
        let key_indices = self.read_uint_array()?;
        let value_offsets = self.read_uint_array()?;
        let base = self.pos;
        let mut obj = IndexMap::with_capacity(key_indices.len());
        for (k_idx, v_off) in key_indices.into_iter().zip(value_offsets.into_iter()) {
            let key = self
                .keys
                .get(k_idx as usize)
                .cloned()
                .unwrap_or_else(|| format!("_key:{}", k_idx));
            self.pos = base + v_off as usize;
            obj.insert(key, self.read_token()?);
        }
        Ok(Value::Object(obj))
    }
}

// Helpers for matching the token-type constant ranges (UINT_8..UINT_32 etc.)
const T_UINT_16: u8 = 14;
const T_UINT_24: u8 = 15;
