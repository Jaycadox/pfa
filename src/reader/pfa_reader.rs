use std::{
    collections::VecDeque,
    io::{Cursor, Read, Seek},
};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::{writer, PfaError};

#[derive(Debug)]
struct PfaHeader {
    version: u8,
    name: String,
    extra_data: Vec<u8>,
}

#[derive(Debug)]
enum PfaSlice {
    Data { offset: u64, size: u64 },
    Catalog { offset: u64, size: u64 },
}

#[derive(Debug)]
struct PfaEntry {
    path: String,
    slice: PfaSlice,
}

#[derive(Debug)]
struct PfaCatalog {
    entries: Vec<PfaEntry>,
}

#[derive(Debug)]
pub struct PfaReader<T: Read + Seek> {
    header: PfaHeader,
    catalog: PfaCatalog,
    data_idx: usize,
    data: T,
}

impl<T: Read + Seek> PfaReader<T> {
    pub fn new(mut input: T) -> Result<Self, PfaError> {
        let header = Self::read_header(&mut input)?;
        let catalog = Self::read_catalog(&mut input)?;

        let data_idx = input.stream_position()? as usize;

        Ok(Self {
            header,
            catalog,
            data_idx,
            data: input,
        })
    }

    pub fn get_file(&mut self, path: &str) -> Option<Vec<u8>> {
        let mut parts = path.split('/').collect::<VecDeque<_>>();
        if parts.is_empty() {
            return None;
        }
        let mut index = 0;
        let mut remaining_size = None;
        let mut part = parts.pop_front()?;
        loop {
            if index == self.catalog.entries.len() {
                return None;
            }

            let needs_data_slice = parts.is_empty(); // the last component of the path would be the
                                                     // file, which would be the only data slice
            let entry = &self.catalog.entries[index];
            remaining_size = remaining_size.map(|x| x - 1);

            if entry.path == part {
                match (&entry.slice, needs_data_slice) {
                    (PfaSlice::Data { offset, size }, true) => {
                        self.data
                            .seek(std::io::SeekFrom::Start(self.data_idx as u64 + offset))
                            .unwrap();
                        let mut buf = vec![0; *size as usize];
                        self.data.read_exact(&mut buf).unwrap();
                        return Some(buf);
                    }
                    (PfaSlice::Catalog { offset, size }, false) => {
                        index += *offset as usize;
                        remaining_size = Some(*size);
                        part = parts.pop_front()?;
                    }
                    _ => {}
                }
            } else {
                index += 1;
            }

            if let Some(0) = remaining_size {
                return None;
            }
        }
    }

    fn read_sized_buffer(buf: &mut T) -> Result<Vec<u8>, PfaError> {
        let size = buf.read_u8()?;
        let mut str_buf = vec![0; size.into()];
        let _ = buf.read(&mut str_buf);
        Ok(str_buf)
    }

    fn read_sized_string(buf: &mut T) -> Result<String, PfaError> {
        let str_buf = Self::read_sized_buffer(buf)?;
        Ok(String::from_utf8(str_buf)?)
    }

    fn read_fixed_sized_string(buf: &mut T, length: usize) -> Result<String, PfaError> {
        let mut string_buf = vec![0; length];
        let _ = buf.read(&mut string_buf)?;

        let string_length = string_buf
            .iter()
            .enumerate()
            .find(|x| *x.1 == 0)
            .map(|(i, _)| i)
            .unwrap_or(length);

        let string_slice = string_buf[0..string_length].to_vec();

        Ok(String::from_utf8(string_slice)?)
    }

    fn read_catalog(buf: &mut T) -> Result<PfaCatalog, PfaError> {
        let num_entries = buf.read_u64::<LittleEndian>()?;
        let mut entries = Vec::with_capacity(num_entries as usize);
        for _ in 0..num_entries {
            entries.push(Self::read_catalog_entry(buf)?);
        }

        let catalog = PfaCatalog { entries };

        Ok(catalog)
    }

    fn read_catalog_entry(buf: &mut T) -> Result<PfaEntry, PfaError> {
        let mut path = Self::read_fixed_sized_string(buf, 32)?; // TODO: don't hardcode this
        let is_directory = path.ends_with('/');
        let slice = if is_directory {
            path = path[0..path.len() - 1].to_string();
            Self::read_catalog_slice(buf)?
        } else {
            Self::read_data_slice(buf)?
        };

        Ok(PfaEntry { path, slice })
    }
    fn read_catalog_slice(buf: &mut T) -> Result<PfaSlice, PfaError> {
        let size = buf.read_u64::<LittleEndian>()?;
        let offset = buf.read_u64::<LittleEndian>()?;

        Ok(PfaSlice::Catalog { offset, size })
    }

    fn read_data_slice(buf: &mut T) -> Result<PfaSlice, PfaError> {
        let size = buf.read_u64::<LittleEndian>()?;
        let offset = buf.read_u64::<LittleEndian>()?;

        Ok(PfaSlice::Data { offset, size })
    }

    fn read_header(buf: &mut T) -> Result<PfaHeader, PfaError> {
        let mut watermark = [0; 3];
        let _ = buf.read(&mut watermark);
        if &watermark != b"pfa" {
            return Err(PfaError::CustomError("invalid watermark".into()));
        }
        let version = buf.read_u8()?;
        let name = Self::read_sized_string(buf)?;
        let extra_data = Self::read_sized_buffer(buf)?;

        let header = PfaHeader {
            version,
            name,
            extra_data,
        };

        Ok(header)
    }
}
