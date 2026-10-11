mod bridge;
mod check;
use gpui_rhai::{
    ActionId, AppManifest, EmbeddedScriptSource, EmbeddedScriptView, KeyBindingSpec, ModuleId,
    ScriptApplication,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn prepare(
    initial: String,
    fixture: Option<serde_json::Value>,
) -> Result<gpui_rhai::PreparedScriptView, gpui_rhai::ScriptViewError> {
    let entry = ModuleId::parse("main").unwrap();
    let mut sources = BTreeMap::from([(entry.clone(), include_str!("../ui/main.rhai").into())]);
    for &(id, source) in gpui_rhai_registry::BUNDLED_COMPONENT_SOURCES_BY_ID {
        if [
            "components/input",
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
    .manifest(manifest)
    .extension(bridge::GitExtension {
        initial,
        fixture,
        gate: Arc::new(Mutex::new(())),
    });
    for (key, action) in [
        ("cmd-s", "alloy.save"),
        ("cmd-r", "alloy.refresh"),
        ("cmd-o", "alloy.open"),
    ] {
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
            Ok(serde_json::json!({"ok":true,"message":"UI check","snapshot":snapshot,"document":document}))
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
    fn renders_real_document_states() {
        let dir = tempfile::tempdir().unwrap();
        alloy::git::run(dir.path(), &["init", "-b", "main"], None).unwrap();
        std::fs::write(dir.path().join("demo.rs"), "fn main() {}\n").unwrap();
        let r = alloy::git::Repository::open(dir.path().to_str().unwrap()).unwrap();
        let snapshot = r.snapshot().unwrap();
        let mut doc = r.document("demo.rs", false).unwrap();
        for mode in 0..4 {
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
            super::check::check(Some(
                json!({"ok":true,"message":"Test","snapshot":snapshot,"document":doc}),
            ))
            .unwrap();
        }
    }
}
