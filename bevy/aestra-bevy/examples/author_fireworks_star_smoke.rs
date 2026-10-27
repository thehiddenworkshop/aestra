//! Rebuild the saved F8 star-path art candidate from its retained wispy control.
#[path = "fireworks/star_smoke_authoring.rs"]
mod star_smoke_authoring;

fn main() -> Result<(), String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    for (name, asset) in star_smoke_authoring::author(&root)? {
        let source = asset.to_pretty_ron().map_err(|e| e.to_string())?;
        std::fs::write(root.join("effects").join(&name), source).map_err(|e| e.to_string())?;
        println!("Authored {name}");
    }
    Ok(())
}
