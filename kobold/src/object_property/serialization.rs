use std::{
    io::{self, Write},
    marker::PhantomData,
};

use anyhow::bail;
use bitflags::bitflags;
use byteorder::{ReadBytesExt, LE};
use flate2::write::ZlibDecoder;

use super::{reader::BitReader, type_list::*, TypeTag, Value};

#[inline]
fn zlib_decompress<W: Write>(data: &[u8], buf: W) -> io::Result<W> {
    let mut decoder = ZlibDecoder::new(buf);
    decoder.write_all(data)?;
    decoder.finish()
}

bitflags! {
    /// Configuration bits to customize serialization
    /// behavior.
    pub struct SerializerFlags: u32 {
        /// A serializer configuration is part of the state
        /// and should be used upon deserializing.
        const STATEFUL_FLAGS = 1 << 0;
        /// Small length prefix values may be compressed
        /// into smaller integer types.
        const COMPACT_LENGTH_PREFIXES = 1 << 1;
        /// Whether enums are encoded as integer values
        /// or human-readable strings.
        const HUMAN_READABLE_ENUMS = 1 << 2;
        /// Whether the serialized state is zlib-compressed.
        const WITH_COMPRESSION = 1 << 3;
        /// Any property with the `DELTA_ENCODE` bit must
        /// always have its value serialized.
        const FORBID_DELTA_ENCODE = 1 << 4;
    }
}

/// Configuration for the [`Deserializer`].
pub struct DeserializerOptions {
    /// The [`SerializerFlags`] to use.
    pub flags: SerializerFlags,
    /// A set of [`PropertyFlags`] for conditionally ignoring
    /// unmasked properties of a type.
    pub property_mask: PropertyFlags,
    /// Whether the shallow encoding strategy is used for
    /// the data.
    pub shallow: bool,
    /// Whether the data is manually compressed.
    pub manual_compression: bool,
    /// A recursion limit for nested data to avoid stack
    /// overflows.
    pub recursion_limit: u8,
}

impl Default for DeserializerOptions {
    fn default() -> Self {
        Self {
            flags: SerializerFlags::empty(),
            property_mask: PropertyFlags::TRANSMIT | PropertyFlags::PRIVILEGED_TRANSMIT,
            shallow: false,
            manual_compression: false,
            recursion_limit: u8::MAX / 2,
        }
    }
}

/// A configurable deserializer for the ObjectProperty binary
/// format, producing [`Value`]s.
pub struct Deserializer<'de, T> {
    reader: BitReader<'de>,
    options: DeserializerOptions,
    _t: PhantomData<T>,
}

macro_rules! impl_read_len {
    ($($de:ident() = $read:ident()),* $(,)*) => {
        $(
            #[inline]
            fn $de(&mut self) -> anyhow::Result<usize> {
                self.reader.realign_to_byte();
                if self
                    .options
                    .flags
                    .contains(SerializerFlags::COMPACT_LENGTH_PREFIXES)
                {
                    self.read_compact_length_prefix()
                } else {
                    self.reader.$read().map(|v| v as usize).map_err(Into::into)
                }
            }
        )*
    };
}

impl<'de, T> Deserializer<'de, T> {
    /// Creates a new deserializer with its configuration.
    ///
    /// No data for deserialization has been loaded at this
    /// point. [`Deserializer::feed_data`] should be called
    /// next.
    pub fn new(options: DeserializerOptions) -> Self {
        Self {
            reader: BitReader::default(),
            options,
            _t: PhantomData,
        }
    }

    fn decompress_data(
        mut data: &'de [u8],
        scratch: &'de mut Vec<u8>,
    ) -> anyhow::Result<BitReader<'de>> {
        let size = data.read_u32::<LE>()? as usize;

        // Decompress into the scratch buffer.
        scratch.clear();
        scratch.reserve(size);
        let decompressed = zlib_decompress(data, scratch)?;

        // Assert correct size expectations.
        if decompressed.len() != size {
            bail!(
                "Compression size mismatch - expected {} bytes, got {}",
                decompressed.len(),
                size
            );
        }

        Ok(BitReader::new(&decompressed[..]))
    }

    pub fn feed_data(
        &mut self,
        mut data: &'de [u8],
        scratch: &'de mut Vec<u8>,
    ) -> anyhow::Result<()> {
        let reader = if self.options.manual_compression {
            let mut reader = Self::decompress_data(data, scratch)?;

            // If configuration flags are stateful, deserialize them.
            if self.options.flags.contains(SerializerFlags::STATEFUL_FLAGS) {
                self.options.flags = SerializerFlags::from_bits_truncate(reader.load_u32()?);
            }

            reader
        } else {
            // If configuration flags are stateful, deserialize them.
            if self.options.flags.contains(SerializerFlags::STATEFUL_FLAGS) {
                self.options.flags = SerializerFlags::from_bits_truncate(data.read_u32::<LE>()?);
            }

            // Determine whether the data is compressed or not.
            if self
                .options
                .flags
                .contains(SerializerFlags::WITH_COMPRESSION)
                && data.read_u8()? != 0
            {
                Self::decompress_data(data, scratch)?
            } else {
                BitReader::new(data)
            }
        };

        self.reader = reader;
        Ok(())
    }

    fn read_compact_length_prefix(&mut self) -> anyhow::Result<usize> {
        let is_large = self.reader.read_bit()?;
        if is_large {
            self.reader
                .read_value_bits(u32::BITS as usize - 1)
                .map_err(Into::into)
        } else {
            self.reader
                .read_value_bits(u8::BITS as usize - 1)
                .map_err(Into::into)
        }
    }

    impl_read_len! {
        // Used for strings, where the length is written as a `u16`.
        read_str_len() = load_u16(),

        // Used for sequences, where the length is written as a `u32`.
        read_seq_len() = load_u32(),
    }

    fn read_str(&mut self) -> anyhow::Result<Vec<u8>> {
        self.read_str_len()
            .and_then(|len| self.reader.read_bytes(len).map_err(Into::into))
    }

    fn read_wstr(&mut self) -> anyhow::Result<Vec<u16>> {
        let len = self.read_str_len()?;

        let mut result = Vec::with_capacity(len);
        for _ in 0..len {
            result.push(self.reader.load_u16()?);
        }

        Ok(result)
    }

    fn deserialize_unsigned_bits(&mut self, n: usize) -> anyhow::Result<u64> {
        self.reader
            .read_value_bits(n)
            .map(|v| v as u64)
            .map_err(Into::into)
    }

    fn deserialize_signed_bits(&mut self, n: usize) -> anyhow::Result<i64> {
        self.deserialize_unsigned_bits(n).map(|v| {
            // Perform sign-extension of the value we got.
            if v & (1 << (n - 1)) != 0 {
                (v as i64) | ((!0) << n)
            } else {
                v as i64
            }
        })
    }
}

macro_rules! check_recursion {
    (let $new_this:ident = $this:ident $($body:tt)*) => {
        $this.options.recursion_limit -= 1;
        if $this.options.recursion_limit == 0 {
            bail!("deserializer recursion limit exceeded");
        }

        let $new_this = $this $($body)*

        $new_this.options.recursion_limit += 1;
    };
}

macro_rules! impl_deserialize {
    ($($de:ident($ty:ty) = $read:ident()),* $(,)*) => {
        $(
            pub(crate) fn $de(&mut self) -> anyhow::Result<$ty> {
                self.reader.$read().map_err(Into::into)
            }
        )*
    };
}

impl<'de, T: TypeTag> Deserializer<'de, T> {
    /// Deserializes an object [`Value`] from previously
    /// loaded data.
    pub fn deserialize(&mut self, types: &mut TypeList) -> anyhow::Result<Value> {
        check_recursion! {
            let this = self;

            let type_def = T::object_identity(this, types)?;
            let res = if let Some(type_def) = type_def {
                todo!()
            } else {
                Value::Empty
            };
        }

        Ok(res)
    }

    impl_deserialize! {
        deserialize_u8(u8)   = load_u8(),
        deserialize_u16(u16) = load_u16(),
        deserialize_u32(u32) = load_u32(),
        deserialize_u64(u64) = load_u64(),

        deserialize_i8(i8)   = load_i8(),
        deserialize_i16(i16) = load_i16(),
        deserialize_i32(i32) = load_i32(),

        deserialize_f32(f32) = load_f32(),
        deserialize_f64(f64) = load_f64(),
    }
}