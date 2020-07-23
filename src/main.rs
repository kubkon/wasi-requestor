use anyhow::{anyhow, Result};
use futures::{future::FutureExt, pin_mut, select};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    collections::HashMap,
};
use structopt::StructOpt;
use ya_agreement_utils::{constraints, ConstraintKey, Constraints};
use ya_requestor_sdk::{commands, CommandList, Image::WebAssembly, Requestor};
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
    module_name: Option<String>,
}

impl Package {
    fn new() -> Self {
        let options = FileOptions::default().compression_method(CompressionMethod::Stored);
        let zip_writer = ZipWriter::new(Cursor::new(Vec::new()));

        Self {
            zip_writer,
            options,
            module_name: None,
        }
    }

    fn add_module_from_path<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let module_name = path
            .as_ref()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let contents = fs::read(path.as_ref())?;
        self.zip_writer
            .start_file(&module_name, self.options.clone())?;
        self.zip_writer.write(&contents)?;
        self.module_name = Some(module_name);

        Ok(())
    }

    fn write<P: AsRef<Path>>(mut self, path: P) -> Result<()> {
        // create manifest
        let comps: Vec<_> = self.module_name.as_ref().unwrap().split('.').collect();
        let manifest = serde_json::json!({
            "id": "custom",
            "name": "custom",
            "entry-points": [{
                "id": comps[0],
                "wasm-path": self.module_name.unwrap(),
            }],
            "mount-points": [{
                "rw": "workdir",
            }]
        });
        self.zip_writer
            .start_file("manifest.json", self.options.clone())?;
        self.zip_writer.write(&serde_json::to_vec(&manifest)?)?;

        let finalized = self.zip_writer.finish()?.into_inner();
        fs::write(path.as_ref(), finalized)?;

        Ok(())
    }
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let _ = dotenv::dotenv().ok();
    let args = Args::from_args();
    pretty_env_logger::init();

    // Workspace
    let workspace = tempfile::tempdir()?;
    log::info!("Workspace created in '{}'", workspace.path().display());

    // Prepare the zip package
    let package_path = workspace.path().join("package.zip");
    let mut package = Package::new();
    package.add_module_from_path(&args.module)?;
    package.write(&package_path)?;

    let requestor = Requestor::new(
        "kubkon-requestor-agent",
        WebAssembly((1, 0, 0).into()),
        ya_requestor_sdk::Package::Archive(package_path)
    )
    .with_max_budget_gnt(5)
    .with_constraints(constraints![
        "golem.inf.mem.gib" > 0.5,
        "golem.inf.storage.gib" > 1.0,
        "golem.com.pricing.model" == "linear",
    ])
    .with_tasks(vec![commands! {
        upload(&args.args[0], format!("/workdir/{}", &args.args[0]));
        run("custom", format!("/workdir/{}", &args.args[0]), format!("/workdir/{}", &args.args[1]));
        download(format!("/workdir/{}", &args.args[1]), &args.args[1]);
    }].into_iter())
    .on_completed(|outputs: HashMap<String, String>| {
        println!("{:#?}", outputs);
    })
    .run().fuse();

    let ctrl_c = actix_rt::signal::ctrl_c().fuse();

    pin_mut!(requestor, ctrl_c);

    select! {
        comp_res = requestor => comp_res,
        _ = ctrl_c => Err(anyhow!("interrupted: ctrl-c detected!")),
    }
}
