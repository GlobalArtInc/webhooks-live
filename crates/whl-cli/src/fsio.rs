use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIR_MODE: u32 = 0o700;

pub fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(PRIVATE_DIR_MODE);
    builder.create(path)
}

pub fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    options.mode(PRIVATE_FILE_MODE);
    options
}

fn temporary_path(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let mut temporary = name.to_os_string();
    temporary.push(format!(".{}.tmp", std::process::id()));
    Ok(path.with_file_name(temporary))
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> io::Result<()> {
    File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> io::Result<()> {
    Ok(())
}

pub fn write_private_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(directory) = directory {
        create_private_dir(directory)?;
    }
    let temporary = temporary_path(path)?;
    let written = (|| {
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let mut file = private_options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    match directory {
        Some(directory) => sync_directory(directory),
        None => sync_directory(Path::new(".")),
    }
}

pub fn open_lock_file(path: &Path) -> io::Result<File> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the lock file is a symbolic link",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let file = private_options()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    verify_same_file(&file, path)?;
    Ok(file)
}

#[cfg(unix)]
fn verify_same_file(file: &File, path: &Path) -> io::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let opened = file.metadata()?;
    let linked = fs::symlink_metadata(path)?;
    if linked.file_type().is_symlink()
        || opened.dev() != linked.dev()
        || opened.ino() != linked.ino()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the lock file changed while it was opened",
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_same_file(_file: &File, _path: &Path) -> io::Result<()> {
    Ok(())
}

pub fn read_limited(path: &Path, limit: u64) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read;

    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("the file is larger than {limit} bytes"),
        ));
    }
    Ok(Some(bytes))
}
