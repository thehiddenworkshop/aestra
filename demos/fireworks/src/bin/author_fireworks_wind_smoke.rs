//! Author the saved wind-haze variant without host-side particle simulation.
#[path = "../wind_smoke_authoring.rs"]
mod wind_smoke_authoring;

fn main() -> Result<(), String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    for (name, asset) in wind_smoke_authoring::author(&root)? {
        std::fs::write(
            root.join("effects").join(&name),
            asset.to_pretty_ron().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        println!("Authored {name}");
    }
    Ok(())
}
