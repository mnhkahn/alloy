mod app_icon;
mod bridge;
mod check;
mod shortcuts;
use gpui_rhai::{
    ActionId, AppManifest, EmbeddedScriptSource, EmbeddedScriptView, KeyBindingSpec, ModuleId,
    ScriptApplication,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn ui_assets() -> Vec<(String, gpui_rhai::AssetData)> {
    gpui_rhai_registry::BUNDLED_ASSET_SOURCES
        .iter()
        .map(|(name, source)| {
            (
                name.trim_end_matches(".svg").to_string(),
                gpui_rhai::AssetData {
                    mime_type: "image/svg+xml".into(),
                    bytes: source.as_bytes().to_vec(),
                },
            )
        })
        .collect()
}

fn prepare(
    initial: String,
    fixture: Option<serde_json::Value>,
) -> Result<gpui_rhai::PreparedScriptView, gpui_rhai::ScriptViewError> {
    let entry = ModuleId::parse("main").unwrap();
    let mut sources = BTreeMap::from([(entry.clone(), include_str!("../ui/main.rhai").into())]);
    sources.insert(
        ModuleId::parse("components/watch_timer").unwrap(),
        include_str!("../ui/watch_timer.rhai").into(),
    );
    for &(id, source) in gpui_rhai_registry::BUNDLED_COMPONENT_SOURCES_BY_ID {
        if [
            "components/tooltip",
            "components/context_menu",
            "components/menu",
            "components/divider",
            "components/input",
            "components/select",
            "components/combobox",
            "components/textarea",
            "components/diff_viewer",
            "components/code_viewer",
        ]
        .contains(&id)
        {
            sources.insert(ModuleId::parse(id).unwrap(), source.into());
        }
    }
    let manifest = AppManifest::new(entry.clone())
        .with_capability("alloy.boot", "*")
        .unwrap()
        .with_capability("alloy.git", "*")
        .unwrap();
    let mut view = EmbeddedScriptView::new(
        entry,
        EmbeddedScriptSource::new(sources),
        gpui_rhai_registry::DEFAULT_THEME,
    )
    .asset_sources(ui_assets())
    .manifest(manifest)
    .extension(bridge::GitExtension {
        initial,
        fixture,
        gate: Arc::new(Mutex::new(())),
    });
    for &(key, action, _, _, _) in shortcuts::SHORTCUTS {
        view = view.key_binding(
            KeyBindingSpec::new(
                key,
                ActionId::parse(action).unwrap(),
                Some("GPUIRhaiHost".into()),
            )
            .unwrap(),
        );
    }
    view.prepare()
}
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.first().is_some_and(|v| v == "--check");
    let initial = args
        .iter()
        .find(|v| !v.starts_with('-'))
        .cloned()
        .unwrap_or_default();
    let fixture = if check && !initial.is_empty() {
        match alloy::git::Repository::open(&initial).and_then(|repo| {
            let snapshot = repo.snapshot()?;
            let document = snapshot.files.first().map(|file| repo.document(&file.path, false)).transpose()?;
            Ok(serde_json::json!({"ok":true,"message":"UI check","snapshot":snapshot,"overview":alloy::projects::Overview::from_snapshot(&snapshot),"document":document}))
        }) {
            Ok(value) => Some(value),
            Err(error) => { eprintln!("Alloy: {error}"); std::process::exit(1); }
        }
    } else {
        None
    };
    if check && let Err(error) = check::check(fixture.clone()) {
        eprintln!("Alloy UI: {error}");
        std::process::exit(1);
    }
    match prepare(initial, fixture) {
        Ok(prepared) if check => {
            drop(prepared);
            println!("Alloy UI prepared successfully");
        }
        Ok(prepared) => {
            if let Err(error) = ScriptApplication::new(prepared)
                .window_size(1440.0, 900.0)
                .window_options(|mut options, _| {
                    app_icon::install();
                    if let Some(titlebar) = options.titlebar.as_mut() {
                        titlebar.title = Some("Alloy".into());
                    }
                    options.window_min_size = Some(gpui_rhai::gpui::size(
                        gpui_rhai::gpui::px(1200.0),
                        gpui_rhai::gpui::px(780.0),
                    ));
                    options
                })
                .run()
            {
                eprintln!("Alloy: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("Alloy: {error}");
            std::process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn ui_prepares() {
        super::prepare(String::new(), None).unwrap();
        super::check::check(None).unwrap();
    }
}

#[cfg(test)]
mod render_tests {
    use serde_json::json;
    #[test]
    fn renders_image_versions() {
        let dir = tempfile::tempdir().unwrap();
        alloy::git::run(dir.path(), &["init", "-b", "main"], None).unwrap();
        image::DynamicImage::new_rgb8(64, 32)
            .save(dir.path().join("image.png"))
            .unwrap();
        let r = alloy::git::Repository::open(dir.path().to_str().unwrap()).unwrap();
        let mut doc = r.document("image.png", false).unwrap();
        for single in [true, false] {
            doc.added = single;
            if !single {
                doc.images.as_mut().unwrap().left = doc.images.as_ref().unwrap().right.clone();
            }
            super::check::check(Some(
                json!({"ok":true,"message":"Test","snapshot":r.snapshot().unwrap(),"overview":alloy::projects::Overview::from_snapshot(&r.snapshot().unwrap()),"document":doc}),
            ))
            .unwrap();
        }
    }
    #[test]
    fn renders_real_document_states() {
        let dir = tempfile::tempdir().unwrap();
        alloy::git::run(dir.path(), &["init", "-b", "main"], None).unwrap();
        std::fs::write(dir.path().join("demo.rs"), "fn main() {}\n").unwrap();
        let r = alloy::git::Repository::open(dir.path().to_str().unwrap()).unwrap();
        let snapshot = r.snapshot().unwrap();
        let mut doc = r.document("demo.rs", false).unwrap();
        for mode in 0..5 {
            if mode == 1 {
                doc.conflict = true;
                doc.ours = "ours\n".into();
                doc.theirs = "theirs\n".into();
                doc.right = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> other\n".into();
            }
            if mode == 2 {
                doc.conflict = false;
                doc.staged = true;
                doc.editable = false;
            }
            if mode == 3 {
                doc.note = "Binary file".into();
            }
            if mode == 4 {
                doc.note.clear();
                doc.staged = false;
                doc.added = false;
                doc.editable = true;
                doc.left = "one\nkeep\nthree\n".into();
                doc.right = "ONE\nkeep\nTHREE\n".into();
                doc.hunks = alloy::git::hunks(&doc.left, &doc.right);
            }
            super::check::check(Some(
                json!({"ok":true,"message":"Test","snapshot":snapshot,"overview":alloy::projects::Overview::from_snapshot(&snapshot),"document":doc}),
            ))
            .unwrap();
        }
    }
}
