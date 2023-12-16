use crate::PfaError;

#[derive(Debug, Clone)]
pub enum DataCompressionType {
    Automatic,
    Forced(bool),
}

#[derive(Debug, Clone)]
pub struct DataFlags {
    compression: DataCompressionType,
}

impl DataFlags {
    const COMPRESSION: u8 = 0b00000001;
    const RESERVED: u8 = 0b11111110;
    pub fn new(compression: DataCompressionType) -> Self {
        Self { compression }
    }

    pub fn no_compression() -> Self {
        Self {
            compression: DataCompressionType::Forced(false),
        }
    }

    pub fn forced_compression() -> Self {
        Self {
            compression: DataCompressionType::Forced(true),
        }
    }

    pub fn auto() -> Self {
        Self {
            compression: DataCompressionType::Automatic,
        }
    }

    pub fn compression_type(&mut self, compression: DataCompressionType) -> &mut Self {
        self.compression = compression;
        self
    }

    pub fn process_content_and_generate_flags(mut self, file_data: &[u8]) -> (Vec<u8>, u8) {
        let mut contents = file_data.to_vec(); // TODO: maybe use Cow, or take contents via mut ref

        let mut already_compressed = false;
        if let DataCompressionType::Automatic = self.compression {
            let compressed_bytes = lz4_flex::compress_prepend_size(&contents);

            if compressed_bytes.len() < contents.len() {
                contents = compressed_bytes;
                already_compressed = true;
                self.compression = DataCompressionType::Forced(true);
            } else {
                self.compression = DataCompressionType::Forced(false);
            }
        }

        let mut bits: u8 = 0;
        match self.compression {
            DataCompressionType::Forced(true) => {
                bits |= DataFlags::COMPRESSION;
                if !already_compressed {
                    contents = lz4_flex::compress_prepend_size(&contents);
                }
            }
            DataCompressionType::Forced(false) => bits &= !DataFlags::COMPRESSION,
            _ => unreachable!(),
        }
        bits |= DataFlags::RESERVED;

        (contents, bits)
    }

    pub fn unprocess_contents_from_flags(
        bitfield: u8,
        contents: &mut Vec<u8>,
    ) -> Result<(), PfaError> {
        if (bitfield & DataFlags::COMPRESSION) != 0 {
            *contents = lz4_flex::decompress_size_prepended(contents)?;
        }

        Ok(())
    }
}

impl Default for DataFlags {
    fn default() -> Self {
        Self {
            compression: DataCompressionType::Forced(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DataCompressionType, DataFlags};

    #[test]
    fn no_compression_test() {
        let data = vec![5; 2000];
        let flags = DataFlags::new(DataCompressionType::Forced(false));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        assert_eq!(data.len(), new_data.len());
        assert_eq!(bitfield, 0b11111110);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn forced_compression_test() {
        let data = vec![5; 2000];
        let flags = DataFlags::new(DataCompressionType::Forced(true));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        assert_ne!(data.len(), new_data.len());
        assert_eq!(bitfield, 0b11111111);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn auto_compression_test() {
        for size in 0..5000 {
            let data = vec![5; size];
            let flags = DataFlags::new(DataCompressionType::Automatic);
            let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

            assert!(
                data.len() >= new_data.len(),
                "automatic compression produced a larger content size"
            );

            let original_data = data;
            DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data).unwrap();
            assert_eq!(original_data, new_data);
        }
    }
}
