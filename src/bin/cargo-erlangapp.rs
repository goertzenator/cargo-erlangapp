extern crate cargo_erlangapp;

fn main() {
    let appdir = std::env::current_dir().unwrap();

    let args_string: Vec<String> = std::env::args().collect();

    cargo_erlangapp::invoke_with_args(&args_string, &appdir);
}
