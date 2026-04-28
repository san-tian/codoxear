fn main() {
    if let Err(err) = codoxear_backend_rs::broker::main_entry() {
        eprintln!("error: {err}");
        std::process::exit(2);
    }
}
