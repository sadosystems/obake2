use client::OBAKE_URL_SCHEME;
use gpui::{AsyncApp, actions};

actions!(
    cli,
    [
        /// Registers the obake:// URL scheme handler.
        RegisterObakeScheme
    ]
);

pub async fn register_obake_scheme(cx: &AsyncApp) -> anyhow::Result<()> {
    cx.update(|cx| cx.register_url_scheme(OBAKE_URL_SCHEME)).await
}
