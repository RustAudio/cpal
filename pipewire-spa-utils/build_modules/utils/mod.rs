use cargo_metadata::camino::Utf8PathBuf;
use cargo_metadata::{Message, Node, Package};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[macro_export]
macro_rules! debug {
    ($($tokens: tt)*) => {
        println!("cargo:warning={}", format!($($tokens)*))
    }
}

pub struct PackageInfo {
    pub src_path: PathBuf,
    pub build_path: PathBuf,
    pub features: Vec<String>,
}

pub fn map_package_info() -> PackageInfo {
    let (package, resolve) = find_dependency(
        "./Cargo.toml", 
        move |package| package.name == "libspa"
    );
    let src_path = package.manifest_path.parent().unwrap().as_str();
    let src_path = PathBuf::from(src_path).join("src");
    let build_path = dependency_build_path(&package.manifest_path, &resolve).unwrap();
    PackageInfo {
        src_path,
        build_path,
        features: resolve.features.clone(),
    }
}

fn find_dependency<F>(manifest_path: &str, filter: F) -> (Package, Node)
where
    F: Fn(&Package) -> bool
{
    let mut cmd = cargo_metadata::MetadataCommand::new();
    let metadata = cmd
        .manifest_path(manifest_path)
        .exec().unwrap();
    let package = metadata.packages
        .iter()
        .find(move |package| filter(package))
        .unwrap()
        .clone();
    let package_id = package.id.clone();
    let resolve = metadata.resolve.as_ref().unwrap().nodes
        .iter()
        .find(move |node| {
            node.id == package_id
        })
        .unwrap()
        .clone();
    (package, resolve)
}

fn dependency_build_path(manifest_path: &Utf8PathBuf, node: &Node) -> Option<PathBuf> {
    let dependency = node.deps.iter()
        .find(move |dependency| dependency.name == "spa_sys")
        .and_then(move |dependency| Some(dependency.pkg.clone()))
        .unwrap();
    let (package, _) = find_dependency(
        manifest_path.as_ref(),
        move |package| package.id == dependency
    );
    let mut command = Command::new("cargo")
        .current_dir(package.manifest_path.parent().unwrap())
        .args(&["check", "--message-format=json", "--quiet"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    command.wait().unwrap();
    let reader = BufReader::new(command.stdout.take().unwrap());
    for message in Message::parse_stream(reader) {
        match message.ok().unwrap() {
            Message::BuildScriptExecuted(script) => {
                if script.package_id.repr.starts_with("path+file://"){
                    return Some(script.out_dir.clone().as_std_path().to_path_buf())
                }
            },
            _ => ()
        }
    }
    
    None
}

pub fn read_source_file(src_path: &PathBuf, file_path: &PathBuf) -> syn::File {
    let path = src_path.join(file_path);
    let mut file = File::open(path)
        .expect("Unable to open file");

    let mut src = String::new();
    file.read_to_string(&mut src)
        .expect("Unable to read file");

    let syntax = syn::parse_file(&src)
        .expect("Unable to parse file");
    syntax
}