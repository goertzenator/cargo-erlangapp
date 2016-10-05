
extern crate serde_json as json;

use std::fs;
use std::path::{Path, PathBuf};
use std::fs::DirEntry;
use std::process;
use std::error::Error;
use std::io::{self, stderr, Write};
use std::convert::From;
use std::result;
use std::fmt::{self, Display};

/// `try!` for `Option`
macro_rules! otry(
    ($e:expr) => (match $e { Some(e) => e, None => return None })
);

// Special OSX link args
// Without them linker throws a fit about NIF API calls.
#[cfg(target_os="macos")]
static DYLIB_LINKER_ARGS: &'static[&'static str] = &["--", "--codegen", "link-args=-flat_namespace -undefined suppress"];

#[cfg(not(target_os="macos"))]
static DYLIB_LINKER_ARGS: &'static[&'static str] = &[];


static BIN_LINKER_ARGS: &'static[&'static str] = &[];



#[derive(Debug)]
enum MsgError {
    Msg(&'static str),
    MsgIo(&'static str, io::Error),
}

use MsgError::*;

impl Error for MsgError {
    fn description(&self) -> &str {
        match self {
            &Msg(s) => s,
            &MsgIo(s, ref _err) => s,
        }
    }
}

impl Display for MsgError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            &Msg(s) =>
                write!(f, "{}", s),
            &MsgIo(s, ref err) =>
                write!(f, "{} ({})", s, err),
        }
    }
}

/// Main entry point into this application.  Invoked by main() and integration tests
pub fn invoke_with_args_str(args: &[&str], appdir: &Path) {
    let args_string: Vec<String> = args.into_iter().cloned().map(From::from).collect();
    invoke_with_args(&args_string, appdir)
}

pub fn invoke_with_args(args: &[String], appdir: &Path)
{
    match ArgsInfo::from_args(&args) {
        Some(ref ai) => invoke(ai, appdir),
        None => usage(),
    }
}


fn usage() {
    writeln!(stderr(), "Usage:").unwrap();
    writeln!(stderr(), "\tcargo-erlangapp build [cargo rustc args]").unwrap();
    writeln!(stderr(), "\tcargo-erlangapp clean [cargo clean args]").unwrap();
    writeln!(stderr(), "\tcargo-erlangapp test [cargo test args]").unwrap();
    process::exit(1);
}



fn invoke(argsinfo: &ArgsInfo, appdir: &Path) {
    match do_command(argsinfo, appdir) {
        Ok(_) => (),
        Err(err) => {
            writeln!(stderr(), "Error: {}", err).unwrap();
            process::exit(1);
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
    for crate_dir in try!(enumerate_crate_dirs(appdir)).iter() {
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
            try!(cargo_command("rustc", rustc_args.as_slice(), crate_dir));

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
                     .map_err(|err| MsgIo("cannot create dest directories in priv/", err)));
            dst_path.push(dst_name);

            // finally, copy the artifact with its new name.
            try!(fs::copy(src_path, dst_path)
                .map_err(|err| MsgIo("cannot copy artifact", err)));
        }
    };

    Ok(())
}

fn linker_args(target: &Target) -> &'static [&'static str] {
    match *target {
        Target::Dylib(_) => DYLIB_LINKER_ARGS,
        Target::Bin(_) => BIN_LINKER_ARGS,
    }
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
        } else if kinds.contains(&"dylib") || kinds.contains(&"cdylib"){
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
                          .map_err(|err| MsgIo("Cannot read crate manifest",err)));

    enumerate_targets_opt(output.stdout.as_slice())
        .ok_or(Msg("Cannot parse crate manifest"))
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
    for crate_dir in try!(enumerate_crate_dirs(appdir)).iter() {
        println!("Testing {}", crate_dir.to_string_lossy());
        try!(cargo_command("test", &argsinfo.cargo_args, crate_dir));
    };
    Ok(())
}

/// Clean all crates, remote artifacts in `priv/`
fn clean_crates(argsinfo: &ArgsInfo, appdir: &Path) -> Result<(), MsgError> {
    // clean all crate dirs
    for crate_dir in try!(enumerate_crate_dirs(appdir)).iter() {
        println!("Cleaning {}", crate_dir.to_string_lossy());
        try!(cargo_command("clean", &argsinfo.cargo_args, crate_dir));
    };

    // clean priv/crates
    let output_dir =  appdir.join("priv").join("crates");
    remove_dir_all_force(output_dir).map_err(|err| MsgIo("can't delete output dir", err))
}

// Remove dir.  The dir being absent is not an error.
fn remove_dir_all_force<P: AsRef<Path>>(path: P) -> io::Result<()> {

    match fs::metadata(path.as_ref()) {
        Err(err) => {
            match err.kind() {
                io::ErrorKind::NotFound => Ok(()),  // not finding is okay (already cleaned)
                _ => Err(err),   // permission error on parent dir?
            }
        },
        Ok(m) => {
            match m.is_dir() {
                true => fs::remove_dir_all(path),
                false => Ok(()),
            }
        },
    }
}

fn cargo_command(cmd: &str, args: &[String], dir: &Path) -> Result<(), MsgError> {
    process::Command::new("cargo")
        .arg(cmd)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|err| MsgIo("cannot start cargo", err))
        .and_then(|status| {
            match status.success() {
                true => Ok(()),
                false => Err(Msg("cargo command failed")),
            }
        })
}


fn enumerate_crate_dirs(appdir: &Path) -> Result<Vec<PathBuf>, MsgError> {

    appdir
        .join("crates")              // :PathBuf
        .read_dir()                  // :Result<ReadDir>
        .map_err(|err|
            MsgIo("Cannot read 'crates' directory", err)
        )
        .map(|dirs|
            dirs.filter_map(result::Result::ok)      // discard Error entries and unwrap
            .filter(is_crate)            // discard non-crate entries
            .map(|x| x.path())           // take whole path
            .collect()
        )
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
            cargo_args: args[2..].into_iter().cloned().collect(),
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
pub fn find_option_value(args: &[String], key: &str) -> Option<String> {
    let mut i = args.iter();
    loop {
        let arg0 = otry!(i.next());
        if arg0.starts_with(key) {
            // check 'key=value'
            match arg0.split('=').nth(1) { // try to get "value"
                Some("") => return i.next().map(Clone::clone), // "key= value"
                Some(x) => return Some(x.to_string()), // "key=value"
                None => {
                    if **arg0 == *key { // "key =.."
                        let arg1 = otry!(i.next());
                        if **arg1 == *"=" { return i.next().map(Clone::clone) } // "key = value"
                        if arg1.starts_with('=') {
                            return arg1.split('=').nth(1).map(From::from) // "key =value"
                        }
                        // something else, drop through and loop
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find_option_value_wrapper(args: &[&str], key: &str) -> Option<String> {
        let argsv: Vec<String> = args.into_iter().cloned().map(From::from).collect();
        find_option_value(&argsv, key)
    }

    #[test]
    fn test_find_option_value() {
        assert_eq!(None, find_option_value_wrapper(&[], "key"));
        assert_eq!(None, find_option_value_wrapper(&["asdfasdfasdfsdf"], "key"));
        assert_eq!(None, find_option_value_wrapper(&["asdfasdfasdfsdf", "sdfsf"], "key"));
        assert_eq!(None, find_option_value_wrapper(&["asdfasdfasdfsdf", "sdfsf", "sdfsdf"], "key"));
        assert_eq!(Some("value".to_string()), find_option_value_wrapper(&["key=value"], "key"));
        assert_eq!(Some("value".to_string()), find_option_value_wrapper(&["key", "=value"], "key"));
        assert_eq!(Some("value".to_string()), find_option_value_wrapper(&["key=", "value"], "key"));
        assert_eq!(Some("value".to_string()), find_option_value_wrapper(&["key", "=", "value"], "key"));
        assert_eq!(None, find_option_value_wrapper(&["key", "value"], "key"));
        assert_eq!(None, find_option_value_wrapper(&["key", "="], "key"));
        assert_eq!(None, find_option_value_wrapper(&["key"], "key"));
        assert_eq!(None, find_option_value_wrapper(&["key="], "key"));
    }
}
