extern crate cargo_erlangapp;

fn main() {
    let appdir = std::env::current_dir().unwrap();
    cargo_erlangapp::invoke_with_args(std::env::args(), &appdir);
}
