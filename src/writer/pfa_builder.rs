use std::collections::VecDeque;

use crate::writer::pfa_writer::*;

use super::PfaError;

enum PfaBuilderPath {
    Directory(Vec<String>),
    File { parts: Vec<String>, name: String },
}

impl From<String> for PfaBuilderPath {
    fn from(mut value: String) -> Self {
        if !value.starts_with('/') {
            value = format!("/{value}");
        }
        let mut parts = value.split('/').map(|x| x.to_string()).collect();
        if value.ends_with('/') {
            return PfaBuilderPath::Directory(parts);
        }

        let name = parts.remove(parts.len() - 1);

        PfaBuilderPath::File { parts, name }
    }
}

pub struct PfaBuilder {
    name: String,
    file_tree: PfaPath,
}

impl PfaBuilder {
    pub fn new(name: &str) -> Self {
        let root = PfaPath::Directory(PfaDirectory::new("", vec![]));
        Self {
            name: name.to_string(),
            file_tree: root,
        }
    }

    pub fn build(self) -> Result<Vec<u8>, PfaError> {
        let writer = PfaWriter::new(&self.name, self.file_tree);
        writer.generate()
    }

    fn get_directory_index_by_name(name: &str, path: &PfaPath) -> Option<usize> {
        match path {
            PfaPath::File(_) => None,
            PfaPath::Directory(dir) => {
                for (i, file) in dir.contents.iter().enumerate() {
                    if let PfaPath::Directory(inner_dir) = file {
                        if inner_dir.name == name {
                            return Some(i);
                        }
                    }
                }
                None
            }
        }
    }

    fn get_directory_from_index(path: &mut PfaPath, index: usize) -> Option<&mut PfaPath> {
        match path {
            PfaPath::File(_) => None,
            PfaPath::Directory(dir) => dir.contents.get_mut(index),
        }
    }

    fn create(&mut self, path: &PfaBuilderPath, data: Option<Vec<u8>>) -> Result<(), PfaError> {
        let mut parts = VecDeque::from(
            match path {
                PfaBuilderPath::File { parts, .. } => parts,
                PfaBuilderPath::Directory(parts) => parts,
            }
            .clone(),
        );

        parts.pop_front(); // pop root

        let mut working_path = &mut self.file_tree;
        for part in parts.iter() {
            let index = Self::get_directory_index_by_name(part, working_path)
                .or_else(|| {
                    if let PfaPath::Directory(dir) = working_path {
                        dir.contents
                            .push(PfaPath::Directory(PfaDirectory::new(part, vec![])));
                        Some(dir.contents.len() - 1)
                    } else {
                        None
                    }
                })
                .ok_or(PfaError::CustomError(
                    "attempt to create directory where folder exists".into(),
                ))?;
            working_path = Self::get_directory_from_index(working_path, index)
                .ok_or(PfaError::CustomError("could not get directory".into()))?;
        }

        if let PfaBuilderPath::File { name, .. } = path {
            let Some(data) = data else {
                return Err(PfaError::CustomError(
                    "attempt to create file with no content".into(),
                ));
            };

            if let PfaPath::Directory(dir) = working_path {
                dir.contents.push(PfaPath::File(
                    PfaFile::new(name.to_owned(), data)
                        .ok_or(PfaError::CustomError("file name too large".into()))?,
                ));
            } else {
                return Err(PfaError::CustomError(
                    "attempt to create file in non directory".into(),
                ));
            }
        }

        Ok(())
    }

    pub fn add_directory(&mut self, path: &str) -> Result<(), PfaError> {
        let mut path = path.to_string();
        if !path.ends_with('/') {
            path.push('/');
        }
        let path = path.into();
        if let PfaBuilderPath::Directory(_) = path {
            self.create(&path, None)?;
            return Ok(());
        }

        Err(PfaError::CustomError(
            "called add_directory but provided a file".into(),
        ))
    }

    pub fn add_file(&mut self, path: &str, content: Vec<u8>) -> Result<(), PfaError> {
        let path = path.to_string();
        let path = path.into();
        if let PfaBuilderPath::File { .. } = path {
            self.create(&path, Some(content))?;
            return Ok(());
        }

        Err(PfaError::CustomError(
            "called add_file but provided a directory".into(),
        ))
    }
}
