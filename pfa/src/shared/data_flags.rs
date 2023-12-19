use std::io::{Cursor, Read, Write};

use aes_gcm::{aead::Aead, AeadCore, KeyInit};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use rand::{RngCore, SeedableRng};

use crate::PfaError;

#[derive(Debug, Clone)]
pub enum DataCompressionType {
    Automatic,
    Forced(bool),
}

#[derive(Debug, Clone)]
pub struct DataFlags {
    compression: DataCompressionType,
    encryption_key: Option<[u8; 32]>,
    error_correction: Option<f32>,
}

impl DataFlags {
    const COMPRESSION: u8 = 0b00000001;
    const ENCRYPTION: u8 = 0b00000010;
    const ERROR_CORRECTION: u8 = 0b00000100;
    const RESERVED: u8 = 0b11111000;
    pub fn new(
        error_correction: Option<f32>,
        encryption_key: Option<[u8; 32]>,
        compression: DataCompressionType,
    ) -> Self {
        Self {
            encryption_key,
            compression,
            error_correction,
        }
    }

    pub fn no_compression() -> Self {
        Self {
            compression: DataCompressionType::Forced(false),
            ..Default::default()
        }
    }

    pub fn forced_compression() -> Self {
        Self {
            compression: DataCompressionType::Forced(true),
            ..Default::default()
        }
    }

    pub fn auto() -> Self {
        Self {
            compression: DataCompressionType::Automatic,
            ..Default::default()
        }
    }

    pub fn compression_type(mut self, compression: DataCompressionType) -> Self {
        self.compression = compression;
        self
    }

    pub fn error_correction(mut self, error_correction_percentage: Option<f32>) -> Self {
        self.error_correction = error_correction_percentage;
        self
    }

    pub fn encryption(mut self, key: Option<[u8; 32]>) -> Self {
        self.encryption_key = key;
        self
    }

    const MAX_CHUNK_SIZE: usize = 255;

    pub(crate) fn process_content_and_generate_flags(mut self, file_data: &[u8]) -> (Vec<u8>, u8) {
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

        if let Some(key) = self.encryption_key {
            bits |= DataFlags::ENCRYPTION;
            let key = aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&key);
            let cipher = aes_gcm::Aes256Gcm::new(key);
            let mut seed = [0; 32];
            rand::rngs::OsRng.fill_bytes(&mut seed);
            let nonce =
                aes_gcm::Aes256Gcm::generate_nonce(&mut rand_chacha::ChaChaRng::from_seed(seed));

            let mut encrypted = cipher
                .encrypt(&nonce, &contents[..])
                .expect("failed to encrypt");

            let mut header = vec![];
            header
                .write_u64::<LittleEndian>(nonce.len() as u64)
                .unwrap();
            header.write_all(nonce.as_slice()).unwrap();
            header.append(&mut encrypted);

            contents = header;
        }

        if let Some(percentage) = self.error_correction {
            let ecc_size = (percentage * Self::MAX_CHUNK_SIZE as f32) as usize;
            let block_size = Self::MAX_CHUNK_SIZE - ecc_size;

            // The first block has hard coded values and stores the ecc size of the following
            // blocks

            let mut header = vec![];
            {
                let mut first_buf = vec![];
                first_buf
                    .write_u64::<LittleEndian>(ecc_size as u64)
                    .unwrap();
                let first_enc = reed_solomon::Encoder::new(4);
                let first_ecc = first_enc.encode(&first_buf);
                header.extend_from_slice(&first_ecc[..]);
            }

            let enc = reed_solomon::Encoder::new(ecc_size);

            for chunk in contents.chunks(block_size) {
                let encoded = enc.encode(chunk);
                header.extend_from_slice(&encoded);
            }

            contents = header;

            bits |= DataFlags::ERROR_CORRECTION;
        }

        bits |= DataFlags::RESERVED;

        (contents, bits)
    }

    pub(crate) fn unprocess_contents_from_flags(
        bitfield: u8,
        mut contents: &mut Vec<u8>,
        key: Option<[u8; 32]>,
    ) -> Result<(), PfaError> {
        if (bitfield & DataFlags::ERROR_CORRECTION) != 0 {
            let mut c = Cursor::new(&contents);

            let all_chunks_len = contents.len() - 12; // first chunk header size
            let num_chunks = all_chunks_len / Self::MAX_CHUNK_SIZE;
            let mut chunk_sizes = vec![Self::MAX_CHUNK_SIZE; num_chunks];
            if all_chunks_len % Self::MAX_CHUNK_SIZE != 0 {
                chunk_sizes.push(all_chunks_len % Self::MAX_CHUNK_SIZE);
            }

            let ecc_size = {
                // Read first header
                let mut first_header = vec![0; 12];
                c.read_exact(&mut first_header).unwrap();
                let dec = reed_solomon::Decoder::new(4);

                let dec_first_header = dec.correct(&first_header, None).unwrap();
                dec_first_header.data().read_u64::<LittleEndian>().unwrap()
            };

            let mut buf = vec![];
            for chunk_size in chunk_sizes {
                let decoder = reed_solomon::Decoder::new(ecc_size as usize);
                let mut chunk_data = vec![0; chunk_size];
                c.read_exact(&mut chunk_data).unwrap();
                let dec_chunk_data = decoder.correct(&chunk_data, None).unwrap();
                buf.extend_from_slice(dec_chunk_data.data());
            }
            *contents = buf;
        }

        if let Some(key) = key {
            if (bitfield & DataFlags::ENCRYPTION) == 0 {
                return Err(PfaError::DecryptUnencryptedFileError);
            }

            let key = aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&key);
            let cipher = aes_gcm::Aes256Gcm::new(key);
            let mut c = Cursor::new(contents);
            let nonce_length = c.read_u64::<LittleEndian>()?;
            let mut nonce = vec![0; nonce_length as usize];
            c.read_exact(&mut nonce)?;
            let data_start = c.position() as usize;

            contents = c.into_inner();

            *contents = cipher
                .decrypt(aes_gcm::Nonce::from_slice(&nonce), &contents[data_start..])
                .map_err(|_| PfaError::FileDecryptError)?;
        } else if (bitfield & DataFlags::ENCRYPTION) != 0 {
            return Err(PfaError::EncryptedFileKeyNotProvided);
        }

        if (bitfield & DataFlags::COMPRESSION) != 0 {
            *contents = lz4_flex::decompress_size_prepended(contents)?;
        }

        Ok(())
    }

    pub fn generate_key() -> [u8; 32] {
        let mut seed = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut seed);

        let mut key = [0; 32];

        rand_chacha::ChaChaRng::from_seed(seed).fill_bytes(&mut key);

        key
    }
}

impl Default for DataFlags {
    fn default() -> Self {
        Self {
            compression: DataCompressionType::Forced(false),
            encryption_key: None,
            error_correction: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DataCompressionType, DataFlags};

    #[test]
    fn no_compression_test() {
        let data = vec![5; 2000];
        let flags = DataFlags::new(None, None, DataCompressionType::Forced(false));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        assert_eq!(data.len(), new_data.len());
        assert_eq!(bitfield, 0b11111000);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, None).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn forced_compression_test() {
        let data = vec![5; 2000];
        let flags = DataFlags::new(None, None, DataCompressionType::Forced(true));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        assert_ne!(data.len(), new_data.len());
        assert_eq!(bitfield, 0b11111001);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, None).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn auto_compression_test() {
        for size in 0..5000 {
            let data = vec![5; size];
            let flags = DataFlags::new(None, None, DataCompressionType::Automatic);
            let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

            assert!(
                data.len() >= new_data.len(),
                "automatic compression produced a larger content size"
            );

            let original_data = data;
            DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, None).unwrap();
            assert_eq!(original_data, new_data);
        }
    }

    #[test]
    fn encryption_test() {
        let data = vec![5; 2000];
        let key = DataFlags::generate_key();
        let flags = DataFlags::new(None, Some(key), DataCompressionType::Forced(false));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, Some(key)).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn encryption_with_compression_test() {
        let data = vec![5; 2000];
        let key = DataFlags::generate_key();
        let flags = DataFlags::new(None, Some(key), DataCompressionType::Forced(true));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, Some(key)).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn error_correction_test() {
        let data = vec![5; 2000];
        let flags = DataFlags::auto().error_correction(Some(0.5));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        for (i, val) in new_data.iter_mut().enumerate() {
            if i % 3 == 0 {
                // remove 20% of the data
                *val = 0;
            }
        }

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, None).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn error_correction_encryption_test() {
        let data = vec![5; 2000];
        let key = DataFlags::generate_key();
        let flags = DataFlags::auto()
            .error_correction(Some(0.5))
            .encryption(Some(key));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        for (i, val) in new_data.iter_mut().enumerate() {
            if i % 3 == 0 {
                // remove 20% of the data
                *val = 0;
            }
        }

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, Some(key)).unwrap();
        assert_eq!(original_data, new_data);
    }
}
