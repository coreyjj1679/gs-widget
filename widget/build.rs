fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").ok().as_deref() == Some("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("ui/app.ico");
        res.compile().expect("嵌入程式圖示失敗");
    }
    slint_build::compile("ui/app.slint").unwrap();
}
