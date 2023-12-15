use std::{
    collections::VecDeque,
    fmt::Display,
    io::{Read, Seek},
};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::PfaError;

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

pub struct PfaPath {
    parts: VecDeque<String>,
}

impl PfaPath {
    pub fn get_name(&self) -> Option<&String> {
        let mut iter = self.parts.iter();
        let mut last = iter.next_back();
        if last.map(|x| x.is_empty()).unwrap_or(false) {
            last = iter.next_back();
        }

        last
    }

    pub fn append(&self, path: impl Into<Self>) -> Option<Self> {
        let mut parts = self.parts.clone();
        let mut new_parts = path.into().parts;
        if parts
            .iter()
            .next_back()
            .map(|x| x.is_empty())
            .unwrap_or(true)
        {
            let _ = parts.pop_back();
        } else {
            return None;
        }
        parts.append(&mut new_parts);

        Some(Self { parts })
    }

    pub fn get_parent(&self) -> Option<Self> {
        let mut parts = self.parts.clone();
        parts.pop_back()?;

        if parts.is_empty() {
            return None;
        }

        Some(Self { parts })
    }

    pub fn get_parts(&self) -> &VecDeque<String> {
        &self.parts
    }

    pub fn is_directory(&self) -> bool {
        self.parts
            .iter()
            .next_back()
            .map(|x| x.is_empty())
            .unwrap_or(false)
    }

    pub fn is_file(&self) -> bool {
        !self.is_directory()
    }
}

impl From<&str> for PfaPath {
    fn from(value: &str) -> Self {
        let parts = value
            .split('/')
            .map(|x| x.to_string())
            .collect::<VecDeque<_>>();
        Self { parts }
    }
}

impl Display for PfaPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string = self
            .parts
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join("/");
        write!(f, "{}", string)
    }
}

impl std::fmt::Debug for PfaPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

pub struct PfaFileContents {
    path: PfaPath,
    contents: Vec<u8>,
}

impl PfaFileContents {
    pub fn get_path(&self) -> &PfaPath {
        &self.path
    }

    pub fn get_contents(&self) -> &[u8] {
        &self.contents
    }

    pub fn get_name(&self) -> String {
        self.get_path()
            .get_name()
            .map(|x| x.to_string())
            .unwrap_or("internal library error: file contents should have name".to_string())
    }
}

pub struct PfaDirectoryContents {
    path: PfaPath,
    contents: Vec<PfaPath>,
}

impl PfaDirectoryContents {
    pub fn get_path(&self) -> &PfaPath {
        &self.path
    }

    pub fn get_contents(&self) -> &[PfaPath] {
        &self.contents
    }

    pub fn get_name(&self) -> String {
        self.get_path()
            .get_name()
            .map(|x| x.to_string())
            .unwrap_or("internal library error: directory contents should have name".to_string())
    }
}

pub enum PfaPathContents {
    File(PfaFileContents),
    Directory(PfaDirectoryContents),
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

    pub fn get_name(&self) -> &str {
        &self.header.name
    }

    pub fn get_version(&self) -> u8 {
        self.header.version
    }

    pub fn get_extra_data(&self) -> &[u8] {
        &self.header.extra_data
    }

    pub fn get_path(&mut self, path: impl Into<PfaPath>) -> Option<PfaPathContents> {
        let path: PfaPath = path.into();
        let is_directory = path.is_directory();

        let mut parts = path.get_parts().clone();

        if is_directory {
            let _ = parts.pop_back(); // remove last empty part
        }

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

            let is_last = parts.is_empty();
            let needs_data_slice = is_last && !is_directory; // the last component of the path would be the
                                                             // file, which would be the only data slice
            let entry = &self.catalog.entries[index];
            remaining_size = remaining_size.map(|x| x - 1);

            if entry.path == part {
                match (&entry.slice, needs_data_slice) {
                    (PfaSlice::Data { offset, size }, true) => {
                        self.data
                            .seek(std::io::SeekFrom::Start(self.data_idx as u64 + offset))
                            .ok()?;
                        let mut buf = vec![0; *size as usize];
                        self.data.read_exact(&mut buf).ok()?;
                        return Some(PfaPathContents::File(PfaFileContents {
                            path,
                            contents: buf,
                        }));
                    }
                    (PfaSlice::Catalog { offset, size }, false) => {
                        if is_last {
                            let index = index + *offset as usize;
                            let catalog_contents =
                                &self.catalog.entries[index..index + *size as usize];

                            let contents = catalog_contents
                                .iter()
                                .map(|x| match &x.slice {
                                    PfaSlice::Data { .. } => {
                                        path.append(PfaPath::from(&x.path[..]))
                                    }
                                    PfaSlice::Catalog { .. } => {
                                        path.append(PfaPath::from(&(format!("{}/", x.path))[..]))
                                    }
                                })
                                .collect::<Option<Vec<_>>>()?;

                            return Some(PfaPathContents::Directory(PfaDirectoryContents {
                                path,
                                contents,
                            }));
                        }

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

    pub fn get_file(&mut self, path: impl Into<PfaPath>) -> Option<PfaFileContents> {
        if let Some(PfaPathContents::File(f)) = self.get_path(path) {
            return Some(f);
        }

        None
    }

    pub fn get_directory(&mut self, path: impl Into<PfaPath>) -> Option<PfaDirectoryContents> {
        let mut path: PfaPath = path.into();
        if !path.is_directory() {
            path = path.append("")?; // append empty part to make it a directory
        }

        if let Some(PfaPathContents::Directory(d)) = self.get_path(path) {
            return Some(d);
        }

        None
    }

    pub fn traverse_files(&mut self, path: impl Into<PfaPath>, callback: fn(PfaFileContents)) {
        let contents = self.get_path(path);
        match contents {
            Some(PfaPathContents::File(f)) => (callback)(f),
            Some(PfaPathContents::Directory(d)) => {
                for path in d.contents {
                    self.traverse_files(path, callback);
                }
            }
            _ => {}
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
