#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(error) = apple_music_downloader::run_server_process().await {
        apple_music_downloader::app_error!("main", "{error}");
        std::process::exit(1);
    }
}
