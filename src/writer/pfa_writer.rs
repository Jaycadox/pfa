use std::io::{Cursor, Seek, SeekFrom, Write};

use byteorder::{LittleEndian, WriteBytesExt};

use crate::PfaError;

#[derive(Debug)]
pub struct PfaFile {
    pub(super) name: String,
    pub(super) contents: Vec<u8>,
}

impl PfaFile {
    pub fn new(name: String, contents: Vec<u8>) -> Option<Self> {
        Some(Self { name, contents })
    }
}

#[derive(Debug)]
pub struct PfaDirectory {
    pub(super) name: String,
    pub(super) contents: Vec<PfaPath>,
}

impl PfaDirectory {
    pub fn new(name: &str, contents: Vec<PfaPath>) -> Self {
        Self {
            name: name.to_string(),
            contents,
        }
    }
}

#[derive(Debug)]
pub enum PfaPath {
    File(PfaFile),
    Directory(PfaDirectory),
}

impl PfaPath {
    const MAX_SIZE: usize = 32;
}

#[derive(Clone, Debug)]
struct PfaDataSlice {
    offset: u64,
    size: u64,
}

#[derive(Clone, Debug)]
struct PfaCatalogSlice {
    index: u64,
    size: u64,
}

pub struct PfaWriter {
    name: String,
    version: u8,
    files: PfaPath,
    buf: Cursor<Vec<u8>>,
    data: Vec<u8>,
}

impl PfaWriter {
    pub fn new(name: &str, files: PfaPath) -> Self {
        Self {
            buf: Cursor::new(vec![]),
            data: vec![],
            files,
            name: name.to_string(),
            version: 1,
        }
    }

    pub fn generate(self) -> Result<Vec<u8>, PfaError> {
        self.write_pfa()
    }

    fn write_u8_sized_string(&mut self, string: &str) -> Result<(), PfaError> {
        self.buf.write_u8(string.len() as u8)?;
        self.buf.write_all(string.as_bytes())?;

        Ok(())
    }

    fn write_nulled_fixed_size_string(
        &mut self,
        string: &str,
        size: usize,
    ) -> Result<(), PfaError> {
        if string.len() > size {
            return Err(PfaError::CustomError(format!(
                "string of length {} is larger than max string size of {}",
                string.len(),
                size
            )));
        }

        self.buf.write_all(string.as_bytes())?;
        let nulls = vec![0; size - string.len()];

        self.buf.write_all(&nulls)?;

        Ok(())
    }

    fn write_pfa(mut self) -> Result<Vec<u8>, PfaError> {
        self.buf.write_all(b"pfa")?; // watermark
        self.write_header()?;
        self.write_catalog()?;
        self.write_data()?;
        Ok(self.buf.into_inner())
    }

    fn write_header(&mut self) -> Result<(), PfaError> {
        self.buf.write_u8(self.version)?; // version
        self.write_u8_sized_string(&self.name.clone())?; // name
        self.buf.write_u8(0)?; // size of extra data

        Ok(())
    }

    fn write_catalog(&mut self) -> Result<(), PfaError> {
        struct CatalogState<'a> {
            writer: &'a mut PfaWriter,
            catalog_len: u64,
            catalog_start: u64,
        }

        let mut file = PfaPath::File(PfaFile::new("".to_string(), vec![]).ok_or(
            PfaError::CustomError("unable to make empty file for swap".to_string()),
        )?);
        std::mem::swap(&mut file, &mut self.files);

        let catalog_len_idx = self.buf.position();
        self.buf.write_u64::<LittleEndian>(0)?;

        let mut catalog_len = 0;
        if let PfaPath::Directory(dir) = &file {
            let name = dir.name.clone();
            let size = dir.contents.len() as u64;
            self.write_catalog_entry(&name, &PfaCatalogSlice { index: 1, size })?;
            catalog_len += 1;
        }
        let catalog_start_idx = self.buf.position();
        const ENTRY_SIZE: usize = 48;
        fn write_catalog_inner(state: &mut CatalogState, path: &PfaPath) -> Result<(), PfaError> {
            match path {
                PfaPath::Directory(dir) => {
                    let mut catalog_idx = vec![];
                    for _ in &dir.contents {
                        catalog_idx.push(state.writer.buf.position());
                        state.writer.buf.write_all(&[0; ENTRY_SIZE])?; // pre allocate catalog
                    }
                    for (idx, path) in catalog_idx.iter().zip(dir.contents.iter()) {
                        match path {
                            PfaPath::Directory(dir) => {
                                let idx = *idx;
                                state.writer.buf.seek(SeekFrom::End(0))?;
                                let end_pos =
                                    (state.writer.buf.position() - idx) / ENTRY_SIZE as u64;
                                write_catalog_inner(state, path)?;
                                state.writer.buf.set_position(idx);
                                state.writer.write_catalog_entry(
                                    &dir.name,
                                    &PfaCatalogSlice {
                                        index: end_pos,
                                        size: dir.contents.len() as u64,
                                    },
                                )?;
                                state.catalog_len += 1;
                                state.writer.buf.seek(SeekFrom::End(0))?;
                            }
                            PfaPath::File(_) => {
                                state.writer.buf.set_position(*idx);
                                write_catalog_inner(state, path)?;
                            }
                        }
                    }
                }
                PfaPath::File(file) => {
                    let data_idx = state.writer.data.len();
                    state.writer.data.append(&mut file.contents.clone());
                    state.writer.write_data_entry(
                        &file.name,
                        &PfaDataSlice {
                            offset: data_idx as u64,
                            size: file.contents.len() as u64,
                        },
                    )?;
                    state.catalog_len += 1;
                }
            };
            Ok(())
        }

        let mut state = CatalogState {
            writer: self,
            catalog_len,
            catalog_start: catalog_start_idx,
        };

        write_catalog_inner(&mut state, &file)?;
        let catalog_len = state.catalog_len;
        self.buf.set_position(catalog_len_idx);
        self.buf.write_u64::<LittleEndian>(catalog_len)?;
        self.buf.seek(SeekFrom::End(0))?;

        Ok(())
    }

    fn write_data_entry(&mut self, filename: &str, slice: &PfaDataSlice) -> Result<(), PfaError> {
        self.write_nulled_fixed_size_string(filename, PfaPath::MAX_SIZE)?;
        self.buf.write_u64::<LittleEndian>(slice.size)?;
        self.buf.write_u64::<LittleEndian>(slice.offset)?;
        Ok(())
    }

    fn write_catalog_entry(
        &mut self,
        filename: &str,
        slice: &PfaCatalogSlice,
    ) -> Result<(), PfaError> {
        self.write_nulled_fixed_size_string(&format!("{}/", filename), PfaPath::MAX_SIZE)?;
        self.buf.write_u64::<LittleEndian>(slice.size)?;
        self.buf.write_u64::<LittleEndian>(slice.index)?;
        Ok(())
    }

    fn write_data(&mut self) -> Result<(), PfaError> {
        self.buf.write_all(&self.data)?;
        Ok(())
    }
}
