use std::sync::LazyLock;

use clap::Parser;
use url::Url;

pub static LONG_VERSION: LazyLock<String> = LazyLock::new(|| {
    format!(
        "
{}
BUILD_GIT_BRANCH={}
BUILD_GIT_COMMIT={}
BUILD_GIT_DATE={}
BUILD_TIME={}",
        env!("CARGO_PKG_VERSION"),
        env!("BUILD_GIT_BRANCH"),
        env!("BUILD_GIT_COMMIT"),
        env!("BUILD_GIT_DATE"),
        env!("BUILD_TIME"),
    )
});

// Acki Nacki proxy publisher CLI
#[derive(Parser, Debug)]
#[command(author, long_version = &**LONG_VERSION, about, long_about = None)]
pub struct CliArgs {
    #[arg(short, long, default_value = "localhost:8090")]
    pub endpoint: Url,
}
