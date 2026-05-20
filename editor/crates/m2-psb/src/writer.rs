// PSB binary writer. Mirrors the layout produced by `PsbWriter` in mzstool.py:
//   - Header (magic, version, flags, offset table, optional checksum)
//   - Key names trie (value_offsets, tree, tails)
//   - Root token (object/array body inline; nested values follow)
//   - Strings table (offsets array + null-terminated UTF-8 blob)
//   - BStreams meta (v4 only — currently always written empty by us)
//   - Streams meta (offsets array + sizes array)
//   - Streams blob (each aligned to STREAM_ALIGNMENT)

use crate::trie::KeyNamesTrie;
use crate::value::{Stream, Value};
use crate::Error;

const STREAM_ALIGNMENT: usize = 32;

const T_NULL: u8 = 1;
const T_FALSE: u8 = 2;
const T_TRUE: u8 = 3;
const T_INT_0: u8 = 4;
const T_INT_8: u8 = 5;
const T_INT_16: u8 = 6;
const T_INT_24: u8 = 7;
const T_INT_32: u8 = 8;
const T_LONG_40: u8 = 9;
const T_UINT_8: u8 = 13;
const T_STRING_BASE: u8 = 21;
const T_STREAM_BASE: u8 = 25;
const T_FLOAT_0: u8 = 29;
const T_FLOAT: u8 = 30;
const T_DOUBLE: u8 = 31;
const T_ARRAY: u8 = 32;
const T_OBJECT: u8 = 33;
const T_BSTREAM_BASE: u8 = 34;

pub fn write(value: &Value, version: u16) -> Result<Vec<u8>, Error> {
    let mut w = Writer::new(version);
    w.write(value)
}

struct Writer {
    version: u16,
    keys: Vec<String>,
    key_lookup: std::collections::HashMap<String, u64>,
    strings: Vec<String>,
    string_lookup: std::collections::HashMap<String, u64>,
    streams: Vec<Stream>,
    bstreams: Vec<Stream>,
}

impl Writer {
    fn new(version: u16) -> Self {
        Self {
            version,
            keys: Vec::new(),
            key_lookup: std::collections::HashMap::new(),
            strings: Vec::new(),
            string_lookup: std::collections::HashMap::new(),
            streams: Vec::new(),
            bstreams: Vec::new(),
        }
    }

    fn write(&mut self, root: &Value) -> Result<Vec<u8>, Error> {
        // Phase 1 — collect keys, strings, streams
        self.collect(root);

        // Phase 2 — sort tables for canonical output
        self.keys.sort();
        self.key_lookup = self.keys.iter().enumerate().map(|(i, k)| (k.clone(), i as u64)).collect();
        self.strings.sort();
        self.string_lookup = self.strings.iter().enumerate().map(|(i, s)| (s.clone(), i as u64)).collect();
        // Streams are sorted by their declared index — gives reproducible output and
        // matches typical (dense) M2 stream numbering.
        self.streams.sort_by_key(|s| s.index);
        self.bstreams.sort_by_key(|s| s.index);

        let trie = KeyNamesTrie::build(&self.keys);

        // Phase 3 — emit binary
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"PSB\0");
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags

        let header_start = buf.len();
        let mut num_offsets = 8;
        if self.version >= 3 { num_offsets += 1; }
        if self.version >= 4 { num_offsets += 3; }
        buf.extend(std::iter::repeat(0u8).take(num_offsets * 4));

        let keys_offsets_offset = buf.len() as u32;
        let keys_blob_offset = buf.len() as u32;
        write_uint_array(&mut buf, &trie.value_offsets);
        write_uint_array(&mut buf, &trie.tree);
        write_uint_array(&mut buf, &trie.tails);

        let root_offset = buf.len() as u32;
        self.write_token(&mut buf, root);

        let strings_offsets_offset = buf.len() as u32;
        let mut string_offsets: Vec<u64> = Vec::with_capacity(self.strings.len());
        let mut string_blob: Vec<u8> = Vec::new();
        for s in &self.strings {
            string_offsets.push(string_blob.len() as u64);
            string_blob.extend_from_slice(s.as_bytes());
            string_blob.push(0);
        }
        write_uint_array(&mut buf, &string_offsets);
        let strings_blob_offset = buf.len() as u32;
        buf.extend_from_slice(&string_blob);

        // BStreams meta (v4) — we always emit empty (no bstream sources from JSON
        // in practice for our use case). The header offsets still point at the
        // empty arrays so readers don't crash.
        let mut bstreams_offsets_offset: u32 = 0;
        let mut bstreams_sizes_offset: u32 = 0;
        let mut bstreams_blob_offset: u32 = 0;
        if self.version >= 4 {
            bstreams_offsets_offset = buf.len() as u32;
            let bs_offsets: Vec<u64> = self.bstreams.iter()
                .scan(0u64, |acc, s| {
                    let cur = *acc;
                    *acc += align_up(s.data.len(), STREAM_ALIGNMENT) as u64;
                    Some(cur)
                })
                .collect();
            write_uint_array(&mut buf, &bs_offsets);
            bstreams_sizes_offset = buf.len() as u32;
            let bs_sizes: Vec<u64> = self.bstreams.iter().map(|s| s.data.len() as u64).collect();
            write_uint_array(&mut buf, &bs_sizes);
            align(&mut buf, STREAM_ALIGNMENT);
            bstreams_blob_offset = buf.len() as u32;
            for s in &self.bstreams {
                buf.extend_from_slice(&s.data);
                align(&mut buf, STREAM_ALIGNMENT);
            }
        }

        // Streams meta + blob
        let streams_offsets_offset = buf.len() as u32;
        let stream_offsets: Vec<u64> = self.streams.iter()
            .scan(0u64, |acc, s| {
                let cur = *acc;
                *acc += align_up(s.data.len(), STREAM_ALIGNMENT) as u64;
                Some(cur)
            })
            .collect();
        write_uint_array(&mut buf, &stream_offsets);
        let streams_sizes_offset = buf.len() as u32;
        let stream_sizes: Vec<u64> = self.streams.iter().map(|s| s.data.len() as u64).collect();
        write_uint_array(&mut buf, &stream_sizes);
        align(&mut buf, STREAM_ALIGNMENT);
        let streams_blob_offset = buf.len() as u32;
        for s in &self.streams {
            buf.extend_from_slice(&s.data);
            align(&mut buf, STREAM_ALIGNMENT);
        }

        // Backfill header
        let mut p = header_start;
        write_u32_at(&mut buf, p, keys_offsets_offset); p += 4;
        write_u32_at(&mut buf, p, keys_blob_offset); p += 4;
        write_u32_at(&mut buf, p, strings_offsets_offset); p += 4;
        write_u32_at(&mut buf, p, strings_blob_offset); p += 4;
        write_u32_at(&mut buf, p, streams_offsets_offset); p += 4;
        write_u32_at(&mut buf, p, streams_sizes_offset); p += 4;
        write_u32_at(&mut buf, p, streams_blob_offset); p += 4;
        write_u32_at(&mut buf, p, root_offset); p += 4;

        if self.version >= 3 {
            // Adler-32 of the 32-byte block above (and the 12-byte bstream block on v4).
            let mut adler_input: Vec<u8> = buf[header_start..header_start + 32].to_vec();
            if self.version >= 4 {
                adler_input.extend_from_slice(&bstreams_offsets_offset.to_le_bytes());
                adler_input.extend_from_slice(&bstreams_sizes_offset.to_le_bytes());
                adler_input.extend_from_slice(&bstreams_blob_offset.to_le_bytes());
            }
            let checksum = adler32(&adler_input);
            write_u32_at(&mut buf, p, checksum); p += 4;
            if self.version >= 4 {
                write_u32_at(&mut buf, p, bstreams_offsets_offset); p += 4;
                write_u32_at(&mut buf, p, bstreams_sizes_offset); p += 4;
                write_u32_at(&mut buf, p, bstreams_blob_offset); let _ = p;
            }
        }

        Ok(buf)
    }

    // ---- Phase 1: walk the tree and accumulate keys/strings/streams ----

    fn collect(&mut self, v: &Value) {
        match v {
            Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) => {}
            Value::String(s) => {
                if !self.string_lookup.contains_key(s) {
                    self.string_lookup.insert(s.clone(), self.strings.len() as u64);
                    self.strings.push(s.clone());
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    self.collect(item);
                }
            }
            Value::Object(obj) => {
                for (k, vv) in obj {
                    if !self.key_lookup.contains_key(k) {
                        self.key_lookup.insert(k.clone(), self.keys.len() as u64);
                        self.keys.push(k.clone());
                    }
                    self.collect(vv);
                }
            }
            Value::Stream(s) => {
                if !self.streams.iter().any(|x| x.index == s.index) {
                    self.streams.push(s.clone());
                }
            }
            Value::BStream(s) => {
                if !self.bstreams.iter().any(|x| x.index == s.index) {
                    self.bstreams.push(s.clone());
                }
            }
        }
    }

    // ---- Phase 3: token tree emission ----

    fn write_token(&self, buf: &mut Vec<u8>, v: &Value) {
        match v {
            Value::Null => buf.push(T_NULL),
            Value::Bool(false) => buf.push(T_FALSE),
            Value::Bool(true) => buf.push(T_TRUE),
            Value::Int(n) => self.write_int(buf, *n),
            Value::Float(f) => self.write_float(buf, *f),
            Value::String(s) => {
                let idx = self.string_lookup[s];
                write_indexed(buf, T_STRING_BASE, idx);
            }
            Value::Stream(s) => {
                // Index in the file = position in self.streams (after sorting).
                let idx = self.streams.iter().position(|x| x.index == s.index).unwrap() as u64;
                write_indexed(buf, T_STREAM_BASE, idx);
            }
            Value::BStream(s) => {
                let idx = self.bstreams.iter().position(|x| x.index == s.index).unwrap() as u64;
                write_indexed(buf, T_BSTREAM_BASE, idx);
            }
            Value::Array(arr) => self.write_array(buf, arr),
            Value::Object(obj) => self.write_object(buf, obj),
        }
    }

    fn write_int(&self, buf: &mut Vec<u8>, n: i64) {
        if n == 0 {
            buf.push(T_INT_0);
            return;
        }
        if (-128..=127).contains(&n) {
            buf.push(T_INT_8);
            buf.push(n as u8);
        } else if (-32_768..=32_767).contains(&n) {
            buf.push(T_INT_16);
            buf.extend_from_slice(&(n as i16).to_le_bytes());
        } else if (-8_388_608..=8_388_607).contains(&n) {
            buf.push(T_INT_24);
            let bytes = (n as i32).to_le_bytes();
            buf.extend_from_slice(&bytes[..3]);
        } else if (-2_147_483_648..=2_147_483_647).contains(&n) {
            buf.push(T_INT_32);
            buf.extend_from_slice(&(n as i32).to_le_bytes());
        } else {
            // Find smallest LONG width 5..8 that round-trips
            let bytes = n.to_le_bytes();
            let mut width = 8usize;
            for w in (5..8).rev() {
                let mut tmp = [0u8; 8];
                tmp[..w].copy_from_slice(&bytes[..w]);
                let signed = i64::from_le_bytes(tmp);
                let sign_extended = sign_extend(signed, w);
                if sign_extended == n {
                    width = w;
                }
            }
            buf.push(T_LONG_40 + (width as u8) - 5);
            buf.extend_from_slice(&bytes[..width]);
        }
    }

    fn write_float(&self, buf: &mut Vec<u8>, f: f64) {
        if f == 0.0 && !f.is_sign_negative() {
            buf.push(T_FLOAT_0);
            return;
        }
        // Lossless policy: only encode as f32 when the round-trip is bit-exact.
        // Python's mzstool tolerates 1e-6 drift here, but that quietly degrades
        // precision for values like -1.84 that originate as DOUBLE.
        let as_f32 = f as f32;
        if (as_f32 as f64).to_bits() == f.to_bits() {
            buf.push(T_FLOAT);
            buf.extend_from_slice(&as_f32.to_le_bytes());
        } else {
            buf.push(T_DOUBLE);
            buf.extend_from_slice(&f.to_le_bytes());
        }
    }

    fn write_array(&self, buf: &mut Vec<u8>, arr: &[Value]) {
        buf.push(T_ARRAY);
        let mut temp: Vec<u8> = Vec::new();
        let mut offsets: Vec<u64> = Vec::with_capacity(arr.len());
        for item in arr {
            offsets.push(temp.len() as u64);
            self.write_token(&mut temp, item);
        }
        write_uint_array(buf, &offsets);
        buf.extend_from_slice(&temp);
    }

    fn write_object(&self, buf: &mut Vec<u8>, obj: &indexmap::IndexMap<String, Value>) {
        buf.push(T_OBJECT);
        // Sort keys alphabetically (PSB convention; matches reader expectations).
        let mut sorted: Vec<(&String, &Value)> = obj.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let key_indices: Vec<u64> = sorted.iter().map(|(k, _)| self.key_lookup[*k]).collect();
        write_uint_array(buf, &key_indices);
        let mut temp: Vec<u8> = Vec::new();
        let mut offsets: Vec<u64> = Vec::with_capacity(sorted.len());
        for (_, v) in &sorted {
            offsets.push(temp.len() as u64);
            self.write_token(&mut temp, v);
        }
        write_uint_array(buf, &offsets);
        buf.extend_from_slice(&temp);
    }
}

// ---- helpers ----

fn write_uint_array(buf: &mut Vec<u8>, arr: &[u64]) {
    if arr.is_empty() {
        buf.push(T_UINT_8);
        buf.push(0);
        buf.push(0); // elem_type=0 → all zeros (count is 0 here)
        return;
    }
    let max = arr.iter().copied().max().unwrap_or(0);
    write_uint_with_type(buf, arr.len() as u64);
    let elem_bytes = bytes_for(max);
    buf.push(T_UINT_8 + elem_bytes - 1);
    for &v in arr {
        let bytes = v.to_le_bytes();
        buf.extend_from_slice(&bytes[..elem_bytes as usize]);
    }
}

fn write_uint_with_type(buf: &mut Vec<u8>, value: u64) {
    let n = bytes_for(value);
    buf.push(T_UINT_8 + n - 1);
    let bytes = value.to_le_bytes();
    buf.extend_from_slice(&bytes[..n as usize]);
}

fn write_indexed(buf: &mut Vec<u8>, base: u8, idx: u64) {
    let n = bytes_for(idx);
    buf.push(base + n - 1);
    let bytes = idx.to_le_bytes();
    buf.extend_from_slice(&bytes[..n as usize]);
}

fn bytes_for(v: u64) -> u8 {
    if v < 256 { 1 }
    else if v < 65_536 { 2 }
    else if v < 16_777_216 { 3 }
    else { 4 }
}

fn write_u32_at(buf: &mut [u8], pos: usize, v: u32) {
    buf[pos..pos + 4].copy_from_slice(&v.to_le_bytes());
}

fn align_up(v: usize, alignment: usize) -> usize {
    (v + alignment - 1) / alignment * alignment
}

fn align(buf: &mut Vec<u8>, alignment: usize) {
    let pad = align_up(buf.len(), alignment) - buf.len();
    buf.extend(std::iter::repeat(0u8).take(pad));
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn sign_extend(v: i64, width: usize) -> i64 {
    if width >= 8 { return v; }
    let mask = (1i64 << (width * 8)) - 1;
    let truncated = v & mask;
    let sign_bit = 1i64 << (width * 8 - 1);
    if truncated & sign_bit != 0 {
        truncated | !mask
    } else {
        truncated
    }
}
