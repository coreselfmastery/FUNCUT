// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Dump every preset as a comp document, which is what a project stores: the
//! shape a template, a slot or an agent edits.
//!
//! ```sh
//! cargo run -p concat-motion --example dump_presets -- /tmp/comps
//! ```

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
