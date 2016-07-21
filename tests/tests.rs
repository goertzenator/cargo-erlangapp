extern crate walkdir;
extern crate itertools;
extern crate cargo_erlangapp;

use cargo_erlangapp::{Target, target_filenames};
use std::ffi::{OsStr};
use std::{env, fs, io};
use std::path::{Path};
use walkdir::WalkDir;
use itertools::Itertools;

#[cfg(unix)]
const TEST_DIR: &'static str = "tests/testdir";
#[cfg(unix)]
const APP_DIR: &'static str = "tests/testdir/testapp";
#[cfg(unix)]
const APP_SRC: &'static str = "tests/testapp";

#[cfg(windows)]
const TEST_DIR: &'static str = "tests\\testdir";
#[cfg(windows)]
const APP_DIR: &'static str = "tests\\testdir\\testapp";
#[cfg(windows)]
const APP_SRC: &'static str = "tests\\testapp";


#[test]
fn do_test() {
    test_init();

    invoke_with_args(&["cargo-erlangapp", "build" ]);
    check_build();
    invoke_with_args(&["cargo-erlangapp", "clean" ]);
    check_clean();

    invoke_with_args(&["cargo-erlangapp", "build", "--release" ]);
    check_build();
    invoke_with_args(&["cargo-erlangapp", "clean" ]);
    check_clean();

    // this test is not portable
    //    invoke_with_args(&["cargo-erlangapp", "build", "--target=x86_64-unknown-linux-gnu" ]);
    //    check_build();
    //    invoke_with_args(&["cargo-erlangapp", "clean" ]);
    //    check_clean();


    invoke_with_args(&["cargo-erlangapp", "test" ]);
    check_clean();
    invoke_with_args(&["cargo-erlangapp", "clean" ]);
    check_clean();

    test_cleanup();
}

fn invoke_with_args(args: &[&str]) {
    let mut appdir = env::current_dir().unwrap();
    appdir.push(TEST_DIR);
    appdir.push("testapp");
    cargo_erlangapp::invoke_with_args(args.into_iter().cloned(), &appdir)
}

fn check_build() {
    check_artifact("bonjourdylib", &Target::Dylib("bonjourdylib".into())).unwrap();
    check_artifact("helloexe", &Target::Bin("helloexe".into())).unwrap();
}

fn check_clean() {
    check_artifact("bonjourdylib", &Target::Dylib("bonjourdylib".into())).unwrap_err();
    check_artifact("helloexe", &Target::Bin("helloexe".into())).unwrap_err();
}

fn check_artifact(cratename: &str, target: &Target) -> Result<String,String> {
    let (dstname, _srcname) = target_filenames(target);
    let targetpath = Path::new(APP_DIR).join("priv").join("crates").join(cratename).join(dstname);
    file_must_exist(&targetpath)
}

fn file_must_exist<S: AsRef<OsStr> + ?Sized>(s: &S) -> Result<String,String> {
    match fs::metadata(s.as_ref())
        .map(|m| m.is_file()).unwrap_or(false) {
        true => Ok(format!("{:?} exists!", s.as_ref())),
        false => Err(format!("{:?} does not exist!", s.as_ref())),
    }
}

fn test_init() {
    test_cleanup();
    copy_all(APP_SRC, TEST_DIR).unwrap();
}


fn test_cleanup() {
    let testdir = Path::new("tests").join("testdir");
    if fs::metadata(&testdir).map(|m| m.is_dir()).unwrap_or(false) {
        fs::remove_dir_all(&testdir).unwrap();
    }
}

fn copy_all<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
    // calculate how many path elements to chop off entry when forming to path
    let chop_cnt = from.as_ref().components().count() - 1;
    for entry in WalkDir::new(from).follow_links(false) {
        let entry = try!(entry);
        let filetype = entry.file_type();
        let compi = entry.path().components().dropping(chop_cnt);
        let to_path = to.as_ref().join(compi.as_path());
        //let to_path = to.as_ref().join(entry.path());
        if filetype.is_dir() {
            try!(fs::create_dir_all(to_path));
        } else if filetype.is_file() {
            try!(fs::copy(entry.path(), to_path));
        }
    }
    Ok(())
}

//testapp_src
//drwxr-xr-x 3 goertzen goertzen 4 Jul 18 09:34 bonjourdylib
//drwxr-xr-x 3 goertzen goertzen 4 Jul 18 09:34 helloexe
//drwxr-xr-x 3 goertzen goertzen 4 Jul 18 09:37 holalib

