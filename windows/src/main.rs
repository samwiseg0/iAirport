#[cfg(windows)]
fn main() {
    std::process::exit(iairport_win::win::app::main());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("iairport-win runs on Windows");
    std::process::exit(2);
}
