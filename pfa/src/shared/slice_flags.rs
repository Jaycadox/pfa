pub enum SliceCompressionType {
    Automatic,
    Forced(bool),
}

pub struct SliceFlags {
    compression: SliceCompressionType,
}

impl SliceFlags {
    const COMPRESSION: u8 = 0b00000001;
    const RESERVED: u8 = 0b11111110;
    pub fn new(compression: SliceCompressionType) -> Self {
        Self { compression }
    }

    pub fn compression_type(&mut self, compression: SliceCompressionType) -> &mut Self {
        self.compression = compression;
        self
    }

    pub fn serialize_flags_and_data(mut self, file_data: &[u8]) -> (Vec<u8>, u8) {
        let mut contents = file_data.to_vec(); // TODO: maybe use Cow

        let mut already_compressed = false;
        if let SliceCompressionType::Automatic = self.compression {
            let compressed_bytes = lz4_flex::compress_prepend_size(&contents);

            if compressed_bytes.len() < contents.len() {
                contents = compressed_bytes;
                already_compressed = true;
                self.compression = SliceCompressionType::Forced(true);
            } else {
                self.compression = SliceCompressionType::Forced(false);
            }
        }

        let mut bits: u8 = 0;
        match self.compression {
            SliceCompressionType::Forced(true) => {
                bits |= SliceFlags::COMPRESSION;
                if !already_compressed {
                    contents = lz4_flex::compress_prepend_size(&contents);
                }
            }
            SliceCompressionType::Forced(false) => bits &= !SliceFlags::COMPRESSION,
            _ => unreachable!(),
        }
        bits |= SliceFlags::RESERVED;

        (contents, bits)
    }
}

#[cfg(test)]
mod tests {
    use super::{SliceCompressionType, SliceFlags};

    #[test]
    fn no_compression_test() {
        let data = vec![5; 2000];
        let flags = SliceFlags::new(SliceCompressionType::Forced(false));
        let (new_data, bitfield) = flags.serialize_flags_and_data(&data);

        assert_eq!(data.len(), new_data.len());
        assert_eq!(bitfield, 0b11111110);
    }

    #[test]
    fn forced_compression_test() {
        let data = vec![5; 2000];
        let flags = SliceFlags::new(SliceCompressionType::Forced(true));
        let (new_data, bitfield) = flags.serialize_flags_and_data(&data);

        assert_ne!(data.len(), new_data.len());
        assert_eq!(bitfield, 0b11111111);
    }

    #[test]
    fn auto_compression_test() {
        for size in 0..5000 {
            let data = vec![5; size];
            let flags = SliceFlags::new(SliceCompressionType::Automatic);
            let (new_data, _) = flags.serialize_flags_and_data(&data);

            assert!(
                data.len() >= new_data.len(),
                "automatic compression produced a larger content size"
            );
        }
    }
}
