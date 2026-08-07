//! Just enough protobuf wire format to write an ONNX file.
//!
//! # Why this is hand-rolled
//!
//! Rust's ONNX ecosystem is almost entirely *import*: `tract`, `ort` and `burn` all read
//! models to run them. Writing one is the rarer direction, and the usual route —
//! `prost` plus `onnx.proto` — drags in `prost-build` and a `protoc` binary at build
//! time. This crate's entire premise is a Deep Q\* agent with no ML framework
//! dependency, and bolting a protobuf compiler onto it to emit one file would trade that
//! away for something the format does not actually require.
//!
//! The ONNX wire format is protobuf, and protobuf's encoding is small: varints, and
//! length-delimited blocks. A `ModelProto` for an MLP needs exactly two wire types. That
//! is this file.
//!
//! # The encoding
//!
//! Every field is preceded by a key varint of `(field_number << 3) | wire_type`:
//!
//! | wire type | meaning                             | used for                    |
//! |-----------|-------------------------------------|-----------------------------|
//! | 0         | varint                              | `int32`, `int64`, enums     |
//! | 2         | length-delimited                    | `string`, `bytes`, messages |
//!
//! Nested messages are just length-delimited blocks of their own encoding, which is why
//! [`Message::field_msg`] can build children independently and splice them in.
//!
//! Proto3 omits fields at their default value. That is not a size optimisation here —
//! ONNX consumers rely on it to distinguish "unset" from "zero" in `oneof` fields like
//! `TensorShapeProto.Dimension`, so the writers below skip zeros deliberately.

/// A protobuf message under construction.
#[derive(Default, Clone)]
pub struct Message {
    buf: Vec<u8>,
}

impl Message {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    /// Base-128 varint, low group first, continuation bit set on all but the last.
    fn write_varint(&mut self, mut value: u64) {
        loop {
            let byte = (value & 0x7F) as u8;
            value >>= 7;
            if value == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    fn write_key(&mut self, field: u32, wire_type: u32) {
        self.write_varint(((field as u64) << 3) | wire_type as u64);
    }

    /// `int64` / `int32` / enum. Negative values encode as their 64-bit two's
    /// complement, which is why the cast to `u64` is correct rather than lossy.
    pub fn field_i64(&mut self, field: u32, value: i64) {
        if value == 0 {
            return; // proto3 default
        }
        self.write_key(field, 0);
        self.write_varint(value as u64);
    }

    /// An `int64` written even when zero — for `oneof` members and required enums,
    /// where omission and zero mean different things.
    pub fn field_i64_always(&mut self, field: u32, value: i64) {
        self.write_key(field, 0);
        self.write_varint(value as u64);
    }

    pub fn field_str(&mut self, field: u32, value: &str) {
        if value.is_empty() {
            return;
        }
        self.field_bytes(field, value.as_bytes());
    }

    /// A `string` written even when empty — repeated string fields must keep their
    /// position, so an empty entry cannot simply vanish.
    pub fn field_str_always(&mut self, field: u32, value: &str) {
        self.write_key(field, 2);
        self.write_varint(value.len() as u64);
        self.buf.extend_from_slice(value.as_bytes());
    }

    pub fn field_bytes(&mut self, field: u32, value: &[u8]) {
        if value.is_empty() {
            return;
        }
        self.write_key(field, 2);
        self.write_varint(value.len() as u64);
        self.buf.extend_from_slice(value);
    }

    /// A nested message. Empty children are skipped, matching proto3.
    pub fn field_msg(&mut self, field: u32, msg: &Message) {
        if msg.is_empty() {
            return;
        }
        self.write_key(field, 2);
        self.write_varint(msg.buf.len() as u64);
        self.buf.extend_from_slice(&msg.buf);
    }

    /// A nested message written even when empty — for messages whose *presence* is the
    /// information, such as a `TypeProto.Tensor` with no shape.
    pub fn field_msg_always(&mut self, field: u32, msg: &Message) {
        self.write_key(field, 2);
        self.write_varint(msg.buf.len() as u64);
        self.buf.extend_from_slice(&msg.buf);
    }

    /// A packed `repeated int64`, as ONNX uses for `dims` and `ints` attributes.
    pub fn field_packed_i64(&mut self, field: u32, values: &[i64]) {
        if values.is_empty() {
            return;
        }
        let mut packed = Message::new();
        for &value in values {
            packed.write_varint(value as u64);
        }
        self.field_bytes(field, packed.as_bytes());
    }
}

/// Encode `f64` weights as little-endian `f32` for a `TensorProto.raw_data` field.
///
/// The network trains in `f64`; ONNX's `FLOAT` tensor type is `f32`, and every runtime
/// supports it while `DOUBLE` support is patchier. The narrowing is the one lossy step
/// in the export, and it is bounded: `f32` carries ~7 significant decimal digits, far
/// past what a Q-value ordering depends on. The validation harness asserts the
/// round-tripped outputs stay within tolerance rather than assuming it.
pub fn f32_raw_data(values: &[f64]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(values.len() * 4);
    for &value in values {
        raw.extend_from_slice(&(value as f32).to_le_bytes());
    }
    raw
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_matches_protobuf_examples() {
        // The canonical examples from the protobuf encoding documentation.
        let mut m = Message::new();
        m.write_varint(1);
        assert_eq!(m.as_bytes(), &[0x01]);

        let mut m = Message::new();
        m.write_varint(300);
        assert_eq!(m.as_bytes(), &[0xAC, 0x02]);

        let mut m = Message::new();
        m.write_varint(0);
        assert_eq!(m.as_bytes(), &[0x00]);
    }

    #[test]
    fn key_packs_field_and_wire_type() {
        // Field 1, wire type 2 => (1 << 3) | 2 = 0x0A, the most common byte in any
        // protobuf payload.
        let mut m = Message::new();
        m.field_str_always(1, "");
        assert_eq!(m.as_bytes()[0], 0x0A);
    }

    #[test]
    fn negative_int64_is_ten_bytes() {
        // Two's complement, not zigzag: -1 is 64 set bits, which is 10 varint groups.
        let mut m = Message::new();
        m.field_i64(1, -1);
        assert_eq!(m.as_bytes().len(), 1 + 10);
    }

    #[test]
    fn proto3_defaults_are_omitted() {
        let mut m = Message::new();
        m.field_i64(1, 0);
        m.field_str(2, "");
        assert!(m.is_empty());
    }

    #[test]
    fn always_variants_write_defaults() {
        let mut m = Message::new();
        m.field_i64_always(1, 0);
        assert_eq!(m.as_bytes(), &[0x08, 0x00]);
    }

    #[test]
    fn packed_int64_wraps_in_one_length_delimited_block() {
        let mut m = Message::new();
        m.field_packed_i64(1, &[1, 300]);
        // key, length=3, then 0x01, 0xAC, 0x02
        assert_eq!(m.as_bytes(), &[0x0A, 0x03, 0x01, 0xAC, 0x02]);
    }

    #[test]
    fn nested_message_is_length_prefixed() {
        let mut child = Message::new();
        child.field_i64_always(1, 7);
        let mut parent = Message::new();
        parent.field_msg(1, &child);
        assert_eq!(parent.as_bytes(), &[0x0A, 0x02, 0x08, 0x07]);
    }

    #[test]
    fn raw_data_is_little_endian_f32() {
        let raw = f32_raw_data(&[1.0]);
        assert_eq!(raw, vec![0x00, 0x00, 0x80, 0x3F]);
        assert_eq!(f32_raw_data(&[1.0, 2.0]).len(), 8);
    }
}
