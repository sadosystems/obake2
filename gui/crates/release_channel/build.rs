fn main() {
    println!("cargo::rustc-check-cfg=cfg(__do_not_set_obake_release_channel)");
    println!("cargo::rerun-if-env-changed=OBAKE_RELEASE_CHANNEL");

    // Enabling this `cfg` will cause a runtime panic if the
    // `OBAKE_RELEASE_CHANNEL` env var is not also set. TLDR: don't set the `cfg`
    // directly, just set the env var (hence the name).
    if std::env::var("OBAKE_RELEASE_CHANNEL").is_ok() {
        println!("cargo::rustc-cfg=__do_not_set_obake_release_channel");
    }
}
