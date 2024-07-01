use std::{
    io::{BufReader, BufWriter, Read, Seek, Write},
    ops::DerefMut,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Context, Result};
use pfa::{builder::PfaBuilder, reader::PfaReader, shared::DataFlags};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

#[derive(Debug)]
struct PfaDiff {
    removed: Vec<String>,
    added: Vec<(String, Vec<u8>)>,
    changed: Vec<(String, String)>,
}

pub fn create_diff(
    mut old: PfaReader<BufReader<impl Read + Seek>>,
    mut new: PfaReader<BufReader<impl Read + Seek>>,
    mut out: BufWriter<impl Write + Seek>,
) -> Result<()> {
    let mut diff = PfaDiff {
        removed: vec![],
        added: vec![],
        changed: vec![],
    };

    // Firstly, look through the old PFA to see if there are any paths which don't exist in the new PFA. These are deleted.
    old.traverse_files_cancelable("/", |file| {
        {
            let path = file.get_path();
            let in_new = new.get_file(&path.to_string()[..], None)?;
            if let Some(new_file) = in_new {
                if file.get_contents() != new_file.get_contents() {
                    // Files with the same path but different content, time to make a patch
                    let old_contents = String::from_utf8(file.get_contents().to_vec())?;
                    let new_contents = String::from_utf8(new_file.get_contents().to_vec())?;
                    let dmp = dmp::Dmp::new();
                    let patches = dmp.patch_make1(&old_contents, &new_contents);
                    let patch_text = dmp.patch_to_text(&patches);
                    diff.changed
                        .push((path.to_string().replace('/', "%"), patch_text));
                }
            } else {
                diff.removed.push(path.to_string().replace('/', "%"));
            }
            anyhow::Ok(())
        }
        .context(format!("scanning file: {}", file.get_path()))
    })
    .context("scanning deleted files")?;

    // Next, traverse new PFA to find files that don't exist in old PFA. These are created and don't need a diff (full content stored)
    new.traverse_files_cancelable("/", |file| {
        {
            let path = file.get_path();
            if old.get_path(&path.to_string()[..], None)?.is_none() {
                diff.added.push((
                    path.to_string().replace('/', "%"),
                    file.get_contents().to_vec(),
                ));
            }
            anyhow::Ok(())
        }
        .context(format!("scanning file: {}", file.get_path()))
    })
    .context("scanning created files")?;

    // Now build a PFA file containing all this information
    let mut builder = PfaBuilder::new(&format!("{}_patch", old.get_name()));
    for remove in &diff.removed {
        builder
            .add_file(&format!("/remove/{}", remove), vec![], DataFlags::auto())
            .context(format!("add 'remove' patch: {}", remove))?;
    }

    for add in &diff.added {
        builder
            .add_file(
                &format!("/add/{}", add.0),
                add.1.to_vec(),
                DataFlags::auto(),
            )
            .context(format!("add 'add' patch: {}", add.0))?;
    }

    for change in &diff.changed {
        builder
            .add_file(
                &format!("/change/{}", change.0),
                change.1.as_bytes().to_vec(),
                DataFlags::auto(),
            )
            .context(format!("add change patch: {}", change.0))?;
    }
    let bytes = builder.build().context("build diff pfa")?;
    out.write_all(&bytes).context("write diff pfa")?;
    out.flush().context("flush diff pfa")?;
    Ok(())
}

pub fn apply_diff(
    mut old: PfaReader<BufReader<impl Read + Seek>>,
    mut diff: PfaReader<BufReader<impl Read + Seek>>,
    mut out: BufWriter<impl Write>,
) -> Result<()> {
    let mut constructed_diff = PfaDiff {
        added: vec![],
        removed: vec![],
        changed: vec![],
    };

    diff.traverse_files("/add/", |file| {
        constructed_diff.added.push((
            file.get_name().replace('%', "/"),
            file.get_contents().to_vec(),
        ));
    });
    diff.traverse_files("/remove/", |file| {
        constructed_diff
            .removed
            .push(file.get_name().replace('%', "/"));
    });

    diff.traverse_files_cancelable("/change/", |file| {
        constructed_diff.changed.push((
            file.get_name().replace('%', "/"),
            String::from_utf8(file.get_contents().to_vec())
                .context("parsing change patch contents as string")?,
        ));
        anyhow::Ok(())
    })?;

    struct ApplyPatchTask {
        patch: String,
        file_contents: String,
        path: String,
    }
    let mut patch_tasks = Vec::with_capacity(constructed_diff.changed.len());

    let mut builder = PfaBuilder::new(&format!("{}_patched", old.get_name()));
    old.traverse_files_cancelable("/", |file| {
        {
            if constructed_diff
                .removed
                .contains(&file.get_path().to_string())
            {
                return anyhow::Ok(());
            }

            if let Some((_, patch)) = constructed_diff
                .changed
                .iter()
                .find(|x| *x.0 == file.get_path().to_string())
            {
                let task = ApplyPatchTask {
                    patch: patch.to_string(),
                    file_contents: String::from_utf8(file.get_contents().to_vec())
                        .context("extracting file contents as utf-8 string")?,
                    path: file.get_path().to_string(),
                };
                patch_tasks.push(task);
            } else {
                builder.add_file(
                    &file.get_path().to_string(),
                    file.get_contents().to_vec(),
                    DataFlags::auto(),
                )?;
            };
            anyhow::Ok(())
        }
        .context(format!("analyzing file: {}", file.get_path()))
    })
    .context("cloning old pfa")?;
    let builder = Arc::new(Mutex::new(builder));

    patch_tasks
        .par_iter()
        .map(|task| {
            {
                let ApplyPatchTask {
                    patch,
                    file_contents,
                    path,
                } = task;

                let dmp = dmp::Dmp::new();
                //dmp.patch_margin = 64;
                let patches = dmp
                    .patch_from_text(patch.to_owned())
                    .map_err(|e| anyhow!("error while deserializing patch: {e:?}"))?;
                let new = dmp
                    .patch_apply(&patches, file_contents)
                    .map_err(|e| anyhow!("error while applying patch: {e:?}"))?;
                if new.1.contains(&false) {
                    return Err(anyhow!("at least 1 patch failed to apply"));
                }

                builder.lock().map_err(|_| anyhow!("get lock"))?.add_file(
                    path,
                    new.0.iter().collect::<String>().as_bytes().to_vec(),
                    DataFlags::auto(),
                )?;
                anyhow::Ok(())
            }
            .context(format!("apply patch for file: {}", task.path))
        })
        .collect::<Result<Vec<_>>>()
        .context("batch apply change patches")?;

    let locked_builder = builder;
    let mut builder = PfaBuilder::new("dummy");
    std::mem::swap(
        locked_builder
            .lock()
            .map_err(|_| anyhow!("get lock"))?
            .deref_mut(),
        &mut builder,
    );

    for add in &constructed_diff.added {
        builder
            .add_file(&add.0, add.1.to_vec(), DataFlags::auto())
            .context(format!("add added file: {}", add.0))?;
    }

    let bytes = builder.build().context("build newly patched pfa")?;
    out.write_all(&bytes).context("write newly patched pfa")?;
    out.flush().context("flush newly patched pfa buffer")?;

    Ok(())
}
