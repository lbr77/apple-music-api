#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(error) = wrapper_rust::run_server_process().await {
        wrapper_rust::app_error!("main", "{error}");
        std::process::exit(1);
    }
}
