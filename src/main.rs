use anyhow::{bail, Result};
use sha3::{Digest, Sha3_512};
use std::{
    fs::{self, File},
    io::{Cursor, Write},
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

#[derive(StructOpt)]
struct Args {
    /// Wasm module
    #[structopt(long)]
    module: PathBuf,

    /// Manifest
    #[structopt(long)]
    manifest: PathBuf,

    /// Yagna WASI workspace
    #[structopt(long)]
    workspace: PathBuf,
}

struct Package {
    zip_writer: ZipWriter<Cursor<Vec<u8>>>,
    options: FileOptions,
}

impl Package {
    fn new() -> Self {
        let options = FileOptions::default().compression_method(CompressionMethod::Stored);
        let zip_writer = ZipWriter::new(Cursor::new(Vec::new()));

        Self {
            zip_writer,
            options,
        }
    }

    fn add_file_from_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        self.zip_writer
            .start_file_from_path(path.as_ref(), self.options.clone())?;
        Ok(())
    }

    fn write<P: AsRef<Path>>(mut self, path: P) -> Result<SealedPackage> {
        let finalized = self.zip_writer.finish()?.into_inner();
        let digest = Sha3_512::digest(&finalized);
        fs::write(path.as_ref(), finalized);

        Ok(SealedPackage {
            path: path.as_ref().to_owned(),
            digest: digest.as_slice().to_owned(),
        })
    }
}

struct SealedPackage {
    path: PathBuf,
    pub digest: Vec<u8>,
}

impl Drop for SealedPackage {
    fn drop(&mut self) {
        fs::remove_file(&self.path).expect("could remove package")
    }
}

fn main() -> Result<()> {
    let args = Args::from_args();
    pretty_env_logger::init();

    // Prepare the zip package
    let mut package = Package::new();
    package.add_file_from_path(&args.module)?;
    package.add_file_from_path(&args.manifest)?;

    // Finalize and get the digest
    let sealed = package.write(args.workspace.join("package.zip"))?;

    Ok(())
}
