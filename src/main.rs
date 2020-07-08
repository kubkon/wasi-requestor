use anyhow::Result;
use generic_array::GenericArray;
use sha3::{Digest, Sha3_512};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use ya_agreement_utils::{constraints, ConstraintKey, Constraints};
use ya_requestor_sdk::{commands, CommandList, Image::WebAssembly, Location::File, Requestor};
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

#[derive(StructOpt)]
struct Args {
    /// Wasm module
    module: PathBuf,

    /// Args
    args: Vec<String>,
}

struct Package {
    zip_writer: ZipWriter<Cursor<Vec<u8>>>,
    options: FileOptions,
    module_path: Option<PathBuf>,
}

impl Package {
    fn new() -> Self {
        let options = FileOptions::default().compression_method(CompressionMethod::Stored);
        let zip_writer = ZipWriter::new(Cursor::new(Vec::new()));

        Self {
            zip_writer,
            options,
            module_path: None,
        }
    }

    fn add_module_from_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        self.module_path = Some(PathBuf::from(path.as_ref().file_name().unwrap()));
        self.zip_writer
            .start_file_from_path(path.as_ref(), self.options.clone())?;
        Ok(())
    }

    fn write<P: AsRef<Path>>(
        mut self,
        path: P,
    ) -> Result<GenericArray<u8, <sha3::Sha3_512 as Digest>::OutputSize>> {
        // create manifest
        let module_name = self
            .module_path
            .as_ref()
            .unwrap()
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        let manifest = serde_json::json!({
            "id": "custom",
            "name": "custom",
            "entry-points": [{
                "id": module_name,
                "wasm-path": self.module_path,
            }],
            "mount-points": [{
                "rw": "workspace"
            }]
        });
        self.zip_writer
            .start_file("manifest.json", self.options.clone())?;
        self.zip_writer.write(&serde_json::to_vec(&manifest)?)?;

        let finalized = self.zip_writer.finish()?.into_inner();
        let digest = Sha3_512::digest(&finalized);
        fs::write(path.as_ref(), finalized)?;

        Ok(digest)
    }
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let args = Args::from_args();
    pretty_env_logger::init();

    // Workspace
    let workspace = tempfile::tempdir()?;
    log::info!("Workspace created in '{}'", workspace.path().display());

    // Prepare the zip package
    let package_path = workspace.path().join("package.zip");
    let mut package = Package::new();
    package.add_module_from_path(&args.module)?;
    let digest = package.write(&package_path)?;
    log::info!("Package digest: '{:x}'", digest);

    let _requestor_actor = Requestor::new(
        "My Requestor",
        WebAssembly((1, 0, 0).into()),
        File(package_path.to_str().unwrap().to_owned()),
    )
    .with_max_budget_gnt(5)
    .with_constraints(constraints![
        "golem.inf.mem.gib" > 0.5,
        "golem.inf.storage.gib" > 1.0
    ])
    .with_tasks(vec![commands! {
        upload(&args.args[0]);
        run("custom", format!("/workspace/{}", &args.args[0]), format!("/workspace/{}", &args.args[1]));
        download(&args.args[1]);
    }].into_iter())
    .on_completed(|outputs: Vec<String>| {
        outputs.iter().enumerate().for_each(|(i, o)| println!("task #{}: {}", i, o));
    })
    .run();

    let _ = actix_rt::signal::ctrl_c().await;
    Ok(())
}
