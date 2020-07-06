use anyhow::{bail, Result};
use sha3::{Digest, Sha3_512};
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
};
use structopt::StructOpt;
use tempfile::tempdir;
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

#[derive(StructOpt)]
struct Args {
    /// Wasm module
    #[structopt(long)]
    module: PathBuf,

    /// Yagna WASI workspace
    #[structopt(long)]
    workspace: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::from_args();
    pretty_env_logger::init();

    let root_dir = tempdir()?;

    // Prepare the zip package
    let buf: Vec<u8> = vec![];
    let w = Cursor::new(buf);
    let mut zipped = ZipWriter::new(w);
    let options = FileOptions::default().compression_method(CompressionMethod::Stored);
    zipped.start_file_from_path(&args.module, options)?;
    let package = zipped.finish()?.into_inner();

    // Compute package's sha3
    let package_digest = Sha3_512::digest(&package);
    println!("digest: {:x}", package_digest);

    // Write package to file
    let package_path = root_dir.path().join("package.zip");
    fs::write(package_path, package)?;

    Ok(())
}
