//! Exercise the actual Rhai lifecycle without requiring a GPU window.
use gpui_rhai::*;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::{Arc, Mutex},
};

pub fn check(fixture: Option<serde_json::Value>) -> Result<(), String> {
    let sources = EmbeddedScriptSource::new(
        gpui_rhai_registry::BUNDLED_COMPONENT_SOURCES_BY_ID
            .iter()
            .map(|(id, source)| (ModuleId::parse(*id).unwrap(), (*source).to_string()))
            .collect(),
    );
    let mut engine = RuntimeEngine::new();
    engine.set_module_resolver(RestrictedModuleResolver::from_source(&sources).map_err(error)?);
    let compiled = engine
        .compile_self_contained_named("main", include_str!("../ui/main.rhai"))
        .map_err(error)?;
    let schema = engine.root_state_schema(&compiled).map_err(error)?;
    let theme = load_theme_source(engine.engine(), "theme", gpui_rhai_registry::DEFAULT_THEME)
        .map_err(error)?;
    let mut runtime = UiRuntimeState::new();
    runtime.theme = Some(
        ThemeManager::from_variants([theme], ThemeSelection::new("Default", "Dark"))
            .map_err(error)?,
    );
    runtime.windows.register_open("main").map_err(error)?;
    crate::bridge::GitExtension {
        initial: String::new(),
        fixture,
        gate: Arc::new(Mutex::new(())),
    }
    .configure_runtime(&mut runtime)?;
    AppManifest::new(ModuleId::parse("main").unwrap())
        .with_capability("alloy.boot", "*")
        .map_err(error)?
        .with_capability("alloy.git", "*")
        .map_err(error)?
        .activate(&mut runtime.capabilities)
        .map_err(error)?;
    let mut life = ScriptLifecycle::new(
        compiled.clone(),
        Rc::new(RefCell::new(runtime)),
        ComponentInstancePath::root("View", "main"),
        Some("main".into()),
        BTreeMap::new(),
        &schema,
    )
    .map_err(error)?
    .with_view_id("main");
    life.start(&mut engine).map_err(error)?;
    // Exercise layout modes, ancestor visibility, editor and confirmation branches.
    for (name, payload) in [
        ("set_mode", UiValue::String("unified".into())),
        ("base_toggle", UiValue::Null),
        ("edit", UiValue::Null),
        ("discard", UiValue::Null),
        ("cancel", UiValue::Null),
    ] {
        let callback = engine.callback(&compiled, name).map_err(error)?;
        let _ = life
            .invoke_callback_transactional(&engine, &callback, payload)
            .map_err(error)?;
        life.render(&mut engine).map_err(error)?;
    }
    Ok(())
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
