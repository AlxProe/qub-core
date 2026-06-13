fn main() {
    let build_config = std::env::var("QUB_BUILD_CONFIG").unwrap_or_else(|_| "mainnet".to_string());
    println!("cargo:rerun-if-env-changed=QUB_BUILD_CONFIG");
    println!("cargo:rustc-env=QUB_BUILD_CONFIG={}", build_config);

    #[cfg(target_os = "windows")]
    {
        use std::env;
        use std::fs::File;
        use std::path::PathBuf;

        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let png_path = manifest_dir.join("assets").join("qubit-coin-logo.png");
        if !png_path.exists() {
            return;
        }

        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let ico_path = out_dir.join("qubit-coin-logo.ico");

        let source = image::open(&png_path).expect("failed to open qubit-coin-logo.png").into_rgba8();
        let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
        for size in [16u32, 24, 32, 48, 64, 128, 256] {
            let resized = image::imageops::resize(&source, size, size, image::imageops::FilterType::Lanczos3);
            let image = ico::IconImage::from_rgba_data(size, size, resized.into_raw());
            icon_dir.add_entry(ico::IconDirEntry::encode(&image).expect("encode ico entry"));
        }
        let mut file = File::create(&ico_path).expect("create ico file");
        icon_dir.write(&mut file).expect("write ico file");

        let is_testnet = build_config.eq_ignore_ascii_case("testnet");
        let product_name = if is_testnet { "Qubit Coin Core Testnet" } else { "Qubit Coin Core" };
        let file_description = if is_testnet { "Qubit Coin Core Testnet" } else { "Qubit Coin Core" };

        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico_path.to_str().expect("ico path str"));
        res.set("ProductName", product_name);
        res.set("FileDescription", file_description);
        res.set("ProductVersion", "1.7.4");
        res.set("FileVersion", "1.7.4.0");
        res.compile().expect("compile windows resources");
    }
}
