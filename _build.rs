use std::io::*;
use std::fs::*;
use winres::*;

fn main() {
    println!("cargo:rerun-if-changed=manifest.xml");
    println!("cargo:rustc-if-env-changed=SOME_VAR");

    let mut manifest = File::open("manifest.xml").expect("manifest.xml not found.");
    let mut data = String::new();
    manifest.read_to_string(&mut data).unwrap();

    let mut wr = WindowsResource::new();
    wr.set("ProductName", "Hello World");
    //wr.set_manifest(data.as_str());
    wr.compile().unwrap();
}
