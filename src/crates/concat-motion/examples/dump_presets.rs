// Dump every preset as a comp document, which is what a project stores.
fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "comps".into());
    std::fs::create_dir_all(&out).unwrap();
    for preset in concat_motion::presets::ALL {
        let comp = concat_motion::presets::build(preset.id).expect("a preset");
        let path = format!("{out}/{}.json", preset.id);
        std::fs::write(&path, serde_json::to_string_pretty(&comp).unwrap()).unwrap();
        println!("{path}  hash {:016x}", comp.hash());
    }
}
