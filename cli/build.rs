// Embeds the LiPi icon (assets/lipi.ico) into lipi.exe on Windows.
fn main() {
    println!("cargo:rerun-if-changed=lipi.rc");
    println!("cargo:rerun-if-changed=../assets/lipi.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let _ = embed_resource::compile("lipi.rc", embed_resource::NONE);
    }
}
