use eyre::{ensure, Context};
use tokio::process::Command;

//use crate::manifest::Manifest;

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Language {
    Cpp,
    CSharp,
    Java,
    Kotlin,
    ObjectiveC,
    Php,
    Pyi,
    Python,
    Ruby,
}

pub async fn generate(language: Language) -> eyre::Result<()> {
    // Uses vendored protoc
    let protoc = protobuf_src::protoc();
    std::env::set_var("PROTOC", protoc.clone());

    //let manifest = Manifest::read().await?;

    let path = "proto";
    //let output = "--cpp_out";

    let status = Command::new(protoc)
        .arg("-I")
        .arg(path)
        .arg("--cpp_out")
        .arg("./cpp")
        .arg("proto/api/demo.proto")
        .status()
        .await
        .wrap_err("failed to run protoc")?;

    ensure!(status.success(), "failed to compile protos using protoc");

    //for ref dependency in manifest.dependencies {
    //let mut path = PathBuf::from(PackageStore::PROTO_DEP_PATH);

    //path.push(dependency.package.packag)

    //tonic_build::configure().compile(
    //&["proto/helloworld/helloworld.proto"],
    //&["proto/helloworld"],
    //)?;
    //}

    Ok(())
}
