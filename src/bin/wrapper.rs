fn main() {
    match wrapper_rust::launcher::run_launcher() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            wrapper_rust::app_error!("launcher", "{error}");
            std::process::exit(1);
        }
    }
}
