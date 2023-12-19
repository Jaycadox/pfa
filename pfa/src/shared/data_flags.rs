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
}

impl DataFlags {
    const COMPRESSION: u8 = 0b00000001;
    const ENCRYPTION: u8 = 0b00000010;
    const RESERVED: u8 = 0b11111100;
    pub fn new(encryption_key: Option<[u8; 32]>, compression: DataCompressionType) -> Self {
        Self {
            encryption_key,
            compression,
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

    pub fn encryption(mut self, key: Option<[u8; 32]>) -> Self {
        self.encryption_key = key;
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

        if let Some(key) = self.encryption_key {
            bits |= DataFlags::ENCRYPTION;
            let key = aes_gcm::Key::<aes_gcm::Aes256Gcm>::from_slice(&key);
            let cipher = aes_gcm::Aes256Gcm::new(key);
            let mut seed = [0; 32];
            rand::rngs::OsRng.fill_bytes(&mut seed);
            let nonce =
                aes_gcm::Aes256Gcm::generate_nonce(&mut rand_chacha::ChaChaRng::from_seed(seed));

            contents = cipher
                .encrypt(&nonce, &contents[..])
                .expect("failed to encrypt");

            let header = vec![];
            let mut c = Cursor::new(header);
            c.write_u64::<LittleEndian>(nonce.len() as u64).unwrap();
            c.write_all(nonce.as_slice()).unwrap();
            let mut header = c.into_inner();
            header.append(&mut contents);

            contents = header;
        }

        bits |= DataFlags::RESERVED;

        (contents, bits)
    }

    pub fn unprocess_contents_from_flags(
        bitfield: u8,
        mut contents: &mut Vec<u8>,
        key: Option<[u8; 32]>,
    ) -> Result<(), PfaError> {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DataCompressionType, DataFlags};

    #[test]
    fn no_compression_test() {
        let data = vec![5; 2000];
        let flags = DataFlags::new(None, DataCompressionType::Forced(false));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        assert_eq!(data.len(), new_data.len());
        assert_eq!(bitfield, 0b11111100);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, None).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn forced_compression_test() {
        let data = vec![5; 2000];
        let flags = DataFlags::new(None, DataCompressionType::Forced(true));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        assert_ne!(data.len(), new_data.len());
        assert_eq!(bitfield, 0b11111101);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, None).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn auto_compression_test() {
        for size in 0..5000 {
            let data = vec![5; size];
            let flags = DataFlags::new(None, DataCompressionType::Automatic);
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
        let flags = DataFlags::new(Some(key), DataCompressionType::Forced(false));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, Some(key)).unwrap();
        assert_eq!(original_data, new_data);
    }

    #[test]
    fn encryption_with_compression_test() {
        let data = vec![5; 2000];
        let key = DataFlags::generate_key();
        let flags = DataFlags::new(Some(key), DataCompressionType::Forced(true));
        let (mut new_data, bitfield) = flags.process_content_and_generate_flags(&data);

        let original_data = data;
        DataFlags::unprocess_contents_from_flags(bitfield, &mut new_data, Some(key)).unwrap();
        assert_eq!(original_data, new_data);
    }
}
