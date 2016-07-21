
extern crate serde_json as json;

use std::fs;
use std::path::{Path, PathBuf};
use std::fs::DirEntry;
use std::process;
use std::error::Error;
use std::io::{stderr, Write};
use std::convert::From;
use std::result;
use std::fmt::{self, Display};

/// `try!` for `Option`
macro_rules! otry(
    ($e:expr) => (match $e { Some(e) => e, None => return None })
);

/// Simple text error type for this application
#[derive(Debug)]
struct MsgError {
    msg: String,
}
impl<'a> From<&'a str> for MsgError {
    fn from(s: &'a str) -> MsgError {
        MsgError{ msg: s.to_string() }
    }
}
impl Display for MsgError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}
impl Error for MsgError {
    fn description(&self) -> &str {
        &self.msg
    }
}

/// Main entry point into this application.  Invoked by main() and integration tests
pub fn invoke_with_args<I>(args: I, appdir: &Path)
    where I: IntoIterator,
          I::Item: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(|x| x.into()).collect();
    ArgsInfo::from_args(&args).map_or_else(usage, |ai|invoke(&ai, appdir));
}

fn usage() {
    writeln!(stderr(), "Usage:").unwrap();
    writeln!(stderr(), "\tcargo-erlangapp build [cargo rustc args]").unwrap();
    writeln!(stderr(), "\tcargo-erlangapp clean [cargo clean args]").unwrap();
    writeln!(stderr(), "\tcargo-erlangapp test [cargo test args]").unwrap();
}



fn invoke(argsinfo: &ArgsInfo, appdir: &Path) {
    match do_command(argsinfo, appdir) {
        Ok(_) => (),
        Err(err) => {
            writeln!(stderr(), "Error: {}", err).unwrap();
        }
    }
}

fn do_command(argsinfo: &ArgsInfo, appdir: &Path) -> Result<(), MsgError> {
    match argsinfo.command {
        CargoCommand::Build =>
            build_crates(argsinfo, appdir),
        CargoCommand::Test =>
            test_crates(argsinfo, appdir),
        CargoCommand::Clean =>
            clean_crates(argsinfo, appdir),
    }
}

fn build_crates(argsinfo: &ArgsInfo, appdir: &Path) -> Result<(), MsgError> {
    // build(rustc) each crate
    for crate_dir in enumerate_crate_dirs(appdir).iter() {
        for target in try!(enumerate_targets(crate_dir)).into_iter() {
            println!("Building {}", crate_dir.to_string_lossy());

            // args for build target
            let mut rustc_args: Vec<String> = match target {
                Target::Bin(ref s) => vec!("--bin".to_string(), s.to_string()),
                Target::Dylib(_) => vec!("--lib".to_string()),  // only 1 lib permitted per crate, name is implicit
            };

            // args from commandline
            rustc_args.extend(argsinfo.cargo_args.iter().cloned());

            // linker args
            rustc_args.extend(linker_args(&target).iter().map(|x|x.to_string()));

            // build it!
            try!(cargo_command("rustc", argsinfo.cargo_args.as_slice(), crate_dir));

            // copy artifacts to priv/crates/<cratename>
            let (dst_name, src_name) = target_filenames(&target);

            // build src path
            let mut src_path = crate_dir.join("target");
            if let Some(ref target_arch) = argsinfo.target {
                src_path.push(target_arch);
            }
            src_path.push( match argsinfo.build_type {
                BuildType::Release => "release",
                _ => "debug",
            });
            src_path.push(src_name);

            // build dst path
            let mut dst_path = appdir.join("priv");
            dst_path.push("crates");
            dst_path.push(crate_dir.file_name().unwrap()); // filename will be valid if rustc worked
            try!(fs::create_dir_all(&dst_path)
                     .map_err(|_| "cannot create dest directories in priv/"));
            dst_path.push(dst_name);

            // finally, copy the artifact with its new name.
            try!(fs::copy(src_path, dst_path)
                .map_err(|_| "cannot copy artifact"));
        }
    };

    Ok(())
}

// special OSX link args
// Without them linker throws a fit about NIF API calls.
#[cfg(target_os="macos")]
fn linker_args(target: &Target) -> &'static [&'static str] {
    match target {
        Target::Dylib(_) =>
            &["--", "--codegen", "link-args='-flat_namespace -undefined suppress'"],
        _ =>
            &[],
    }
}

#[cfg(not(target_os="macos"))]
fn linker_args(_target: &Target) -> &'static [&'static str] {
   &[]
}

/// OS X naming
///
/// Dylibs have `lib` prefix, and `.dylib` suffix gets changed to `.so`.
#[cfg(target_os="macos")]
pub fn target_filenames(target: &Target) -> (String, String) {
    match *target {
        Target::Bin(ref s) => (s.to_string(), s.to_string()),
        Target::Dylib(ref s) => ("lib".to_string() + s + ".so", "lib".to_string() + s + ".dylib"),
    }
}
/// Windows naming
///
/// Bins have `.exe` suffix, dylibs have `.dll` suffix.
#[cfg(windows)]
pub fn target_filenames(target: &Target) -> (String, String) {
    match *target {
        Target::Bin(ref s) => (s.to_string() + ".exe", s.to_string() + ".exe"),
        Target::Dylib(ref s) => (s.to_string() + ".dll", s.to_string() + ".dll"),
    }
}

/// Non-windows, non-OSX nameing
///
/// Dylibs have `lib` prefix and `.so` suffix.
#[cfg(all(unix, not(target_os="macos")))]
pub fn target_filenames(target: &Target) -> (String, String) {
    match *target {
        Target::Bin(ref s) => (s.to_string(), s.to_string()),
        Target::Dylib(ref s) => ("lib".to_string() + s + ".so", "lib".to_string() + s + ".so"),
    }
}


/// Build artifact types
#[derive(Debug)]
pub enum Target {
    Bin(String),
    Dylib(String),
}

impl AsRef<String> for Target {
    fn as_ref(&self) -> &String {
        match *self {
            Target::Bin(ref s) => s,
            Target::Dylib(ref s) => s,
        }
    }
}

impl Target {
    /// Create target from cargo manifest fragment
    fn from_json(obj: &json::Value) -> Option<Target> {
        let name = otry!(obj.find("name")
                    .and_then(|s| s.as_string())
                    .map(|s| s.to_string()));
        let kinds: Vec<&str> = otry!(obj.find("kind")
            .and_then(|s| s.as_array())
            .map(|arr| arr.iter()
                .filter_map( |s| s.as_string())
                .collect()));

        if kinds.contains(&"bin") {
            Some(Target::Bin(name))
        } else if kinds.contains(&"dylib") {
            Some(Target::Dylib(name))
        } else {
            None
        }
    }
}

/// Read manifest for given crate and enumerate targets
fn enumerate_targets(crate_dir: &Path) -> Result<Vec<Target>, MsgError> {
    let output = try!(process::Command::new("cargo").arg("read-manifest")
                          .current_dir(crate_dir)
                          .output()
                          .map_err(|_|"Cannot read crate manifest"));

    enumerate_targets_opt(output.stdout.as_slice())
        .ok_or(MsgError::from("Cannot parse crate manifest"))
}
/// Parse "targets" portion of JSON text to extract targets
fn enumerate_targets_opt(json_slice: &[u8]) -> Option<Vec<Target>> {
    let value: json::Value = otry!(json::from_slice(json_slice).ok());
    value.find("targets")
        .and_then(|v| v.as_array())   // :Option<Vec<Value>>
        .map(|targets|
                 targets
                     .iter()
                     .filter_map(Target::from_json)  // :Vec<Target>
                     .collect())
}

/// Test all crates
fn test_crates(argsinfo: &ArgsInfo, appdir: &Path) -> Result<(), MsgError> {
    // test each create, short circuit fail
    for crate_dir in enumerate_crate_dirs(appdir).iter() {
        println!("Testing {}", crate_dir.to_string_lossy());
        try!(cargo_command("test", &argsinfo.cargo_args, crate_dir));
    };
    Ok(())
}

/// Clean all crates, remote artifacts in `priv/`
fn clean_crates(argsinfo: &ArgsInfo, appdir: &Path) -> Result<(), MsgError> {
    // clean all crate dirs
    for crate_dir in enumerate_crate_dirs(appdir).iter() {
        println!("Cleaning {}", crate_dir.to_string_lossy());
        try!(cargo_command("clean", &argsinfo.cargo_args, crate_dir));
    };

    // clean priv/crates
    let output_dir =  appdir.join("priv").join("crates");
    remove_dir_all_force(output_dir).map_err(|_|"can't delete output dir".into())
}

// Remove dir.  The dir being absent is not an error.
fn remove_dir_all_force<P: AsRef<Path>>(path: P) -> Result<()> {
    fs::metadata(s.as_ref())
        .and_then(|m|
                      if m.dir() {
                          fs::remove_dir_all(p)
                      } else {
                          Ok(())
                      }
        )
}

fn cargo_command(cmd: &str, args: &[String], dir: &Path) -> Result<(), MsgError> {
    process::Command::new("cargo")
        .arg(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|_| "cannot start cargo".into())
        .and_then(|status| {
            match status.success() {
                true => Ok(()),
                false => Err("cargo command failed".into()),
            }
        })
}


fn enumerate_crate_dirs(appdir: &Path) -> Vec<PathBuf> {
    appdir
        .join("crates")              // :PathBuf
        .read_dir()                  // :Result<ReadDir>
        .into_iter().flat_map(|x|x)  // :ReadDir
        .filter_map(result::Result::ok)      // discard Error entries and unwrap
        .filter(is_crate)            // discard non-crate entries
        .map(|x| x.path())           // take whole path
        .collect()
}

fn is_crate(dirent: &DirEntry) -> bool {
    let mut toml_path = dirent.path();
    toml_path.push("Cargo.toml");
    toml_path
        .metadata()             // :Result<Metadata>
        .map(|x| x.is_file())   // :Result<bool>
        .unwrap_or(false)
}

#[derive(Debug)]
enum CargoCommand { Build, Test, Clean }
#[derive(Debug)]
enum BuildType { Release, Debug, DefaultDebug }
#[derive(Debug)]
pub struct ArgsInfo {
    command: CargoCommand,
    target: Option<String>,
    build_type: BuildType,
    cargo_args: Vec<String>,
}

impl ArgsInfo {
    pub fn from_args(args: &[String]) -> Option<ArgsInfo> {
        if args.len() < 2 {
            return None;
        }

        let build_type =
        if find_option(args, "--release") { BuildType::Release }
            else if find_option(args, "--debug") { BuildType::Debug }
            else { BuildType::DefaultDebug };

        Some(ArgsInfo {
            command: otry!(parse_cmd_name(args[1].as_str())),
            target: find_option_value(&args[2..], "--target").map(Into::into),
            build_type: build_type,
            cargo_args: args[2..].iter().cloned().collect(),
        })
    }
}

fn parse_cmd_name(arg: &str) -> Option<CargoCommand> {
    match arg {
        "build" => Some(CargoCommand::Build),
        "test" => Some(CargoCommand::Test),
        "clean" => Some(CargoCommand::Clean),
        _ => None,
    }
}

fn find_option(args: &[String], key: &str) -> bool {
    args.iter().any(|x| **x == *key)
}

/// Search args for "key=value", "key= value", "key =value", or "key = value"
pub fn find_option_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    let mut i = args.iter();
    loop {
        let arg0 = otry!(i.next());
        if arg0.starts_with(key) {
            // check 'key=value'
            match arg0.split('=').nth(1) { // try to get "value"
                Some("") => return i.next().map(|x|x.as_str()), // "key= value"
                Some(x) => return Some(x), // "key=value"
                None => {
                    if **arg0 == *key { // "key =.."
                        let arg1 = otry!(i.next());
                        if **arg1 == *"=" { return i.next().map(|x|x.as_str()) } // "key = value"
                        if arg1.starts_with('=') {
                            return arg1.split('=').nth(1) // "key =value"
                        }
                        // something else, drop through and loop
                    }
                }
            }
        }
    }
}

// attempt at generic version above.  Keeping for further work.
//// search args for "key=value", "key= value", "key =value", or "key = value"
////fn find_option_value<'a>(args: &'a [&str], key: &str) -> Option<&'a str> {
//pub fn find_option_value<'a, I>(args: I, key: &str) -> Option<&'a str>
//    where I: IntoIterator,
//          I::Item: 'a + AsRef<str> {
//    let mut i = args.into_iter().map(|x| x.as_ref());
//    loop {
//        let arg0i = otry!(i.next());
//        let arg0 = arg0i.as_ref();
//        if arg0.starts_with(key) {
//            // check 'key=value'
//            match arg0.split('=').nth(1) { // try to get "value"
//                Some("") => return i.next().map(|x|x.as_ref()), // "key= value"
//                Some(x) => return Some(x), // "key=value"
//                None => {
//                    if arg0 == key { // "key =.."
//                        let arg1i = otry!(i.next());
//                        let arg1 = arg1i.as_ref();
//                        if arg1 == "=" { return i.next().map(|x|x.as_ref()) } // "key = value"
//                        if arg1.starts_with('=') {
//                            return arg1.split('=').nth(1) // "key =value"
//                        }
//                        // something else, drop through and loop
//                    }
//                }
//            }
//        }
//    }
//}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_option_value() {
        assert_eq!(None, find_option_value(&[], "key"));
        assert_eq!(None, find_option_value(&["asdfasdfasdfsdf".to_string()], "key"));
        assert_eq!(None, find_option_value(&["asdfasdfasdfsdf".to_string(), "sdfsf".to_string()], "key"));
        assert_eq!(None, find_option_value(&["asdfasdfasdfsdf".to_string(), "sdfsf".to_string(), "sdfsdf".to_string()], "key"));
        assert_eq!(Some("value"), find_option_value(&["key=value".to_string()], "key"));
        assert_eq!(Some("value"), find_option_value(&["key".to_string(), "=value".to_string()], "key"));
        assert_eq!(Some("value"), find_option_value(&["key=".to_string(), "value".to_string()], "key"));
        assert_eq!(Some("value"), find_option_value(&["key".to_string(), "=".to_string(), "value".to_string()], "key"));
        assert_eq!(None, find_option_value(&["key".to_string(), "value".to_string()], "key"));
        assert_eq!(None, find_option_value(&["key".to_string(), "=".to_string()], "key"));
        assert_eq!(None, find_option_value(&["key".to_string()], "key"));
        assert_eq!(None, find_option_value(&["key=".to_string()], "key"));
    }
}
