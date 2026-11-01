fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for (name, cargo_name) in [
        ("AESTRA_DEMO_PROFILE", "PROFILE"),
        ("AESTRA_DEMO_OPT_LEVEL", "OPT_LEVEL"),
    ] {
        println!(
            "cargo:rustc-env={name}={}",
            std::env::var(cargo_name).expect("Cargo build metadata")
        );
    }
}
