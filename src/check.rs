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
            .chain(std::iter::once((
                ModuleId::parse("components/watch_timer").unwrap(),
                include_str!("../ui/watch_timer.rhai").to_string(),
            )))
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
    runtime
        .assets
        .register(
            "app",
            InMemoryAssetProvider::new(crate::ui_assets().into_iter().collect()),
        )
        .map_err(error)?;
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
    let runtime = Rc::new(RefCell::new(runtime));
    let state_path = ComponentInstancePath::root("View", "main");
    let mut life = ScriptLifecycle::new(
        compiled.clone(),
        runtime.clone(),
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
        ("image_zoom", UiValue::Null),
        ("image_zoom", UiValue::Null),
        ("base_toggle", UiValue::Null),
        ("edit", UiValue::Null),
        ("discard", UiValue::Null),
        ("cancel", UiValue::Null),
        ("mode_open", UiValue::Bool(true)),
        ("mode_open", UiValue::Bool(false)),
        ("whitespace_open", UiValue::Bool(true)),
        ("whitespace_change", UiValue::String("ignore_all".into())),
        ("acp_select_open_changed", UiValue::Bool(true)),
        ("acp_select_open_changed", UiValue::Bool(false)),
        ("acp_dropdown_change", UiValue::String("register".into())),
        ("acp_page_close", UiValue::Null),
    ] {
        let callback = engine.callback(&compiled, name).map_err(error)?;
        let _ = life
            .invoke_callback_transactional(&engine, &callback, payload)
            .map_err(error)?;
        life.render(&mut engine).map_err(error)?;
    }
    let get = |field: &str| {
        runtime
            .borrow()
            .component_state
            .get(&state_path, field)
            .cloned()
            .unwrap()
    };
    if let UiValue::Map(doc) = get("doc") {
        if let Some(UiValue::Map(images)) = doc.get("images") {
            for side in ["left", "right"] {
                if let Some(UiValue::Map(preview)) = images.get(side) {
                    assert_eq!(
                        preview["error"],
                        UiValue::String(String::new()),
                        "image must load before rendering"
                    );
                    if let UiValue::String(asset) = &preview["asset"] {
                        runtime
                            .borrow()
                            .assets
                            .load_image(&AssetId::parse(asset).unwrap())
                            .unwrap();
                    }
                }
            }
        }
        let snapshot = get("snapshot");
        let path = doc["path"].clone();
        for (name, payload) in [
            ("projects_toggle", UiValue::Bool(true)),
            (
                "projects_loaded",
                UiValue::Map(BTreeMap::from([("projects".into(), get("projects"))])),
            ),
            ("projects_toggle", UiValue::Bool(false)),
            ("project_add", UiValue::Null),
            ("project_add", UiValue::Null),
        ] {
            let callback = engine.callback(&compiled, name).map_err(error)?;
            let _ = life
                .invoke_callback_transactional(&engine, &callback, payload)
                .map_err(error)?;
            life.render(&mut engine).map_err(error)?;
        }
        let call =
            |life: &mut ScriptLifecycle, name: &str, payload: UiValue| -> Result<(), String> {
                let callback = engine.callback(&compiled, name).map_err(error)?;
                let _ = life
                    .invoke_callback_transactional(&engine, &callback, payload)
                    .map_err(error)?;
                Ok(())
            };
        // A refresh completed for an older UI context must not be applied or acknowledged.
        let root = if let UiValue::Map(fields) = &snapshot {
            fields["root"].clone()
        } else {
            UiValue::Null
        };
        let response = |epoch| {
            UiValue::Map(BTreeMap::from([
                ("watch_root".into(), root.clone()),
                ("watch_epoch".into(), epoch),
                ("revision".into(), UiValue::Integer(99)),
                ("update".into(), UiValue::Null),
            ]))
        };
        let seen = get("watch_seen");
        call(&mut life, "watch_loaded", response(UiValue::Integer(-1)))?;
        assert_eq!(get("watch_seen"), seen);
        call(
            &mut life,
            "draft_change",
            UiValue::String("unsaved auto-refresh draft".into()),
        )?;
        call(&mut life, "watch_loaded", response(get("watch_epoch")))?;
        assert_eq!(
            get("draft"),
            UiValue::String("unsaved auto-refresh draft".into())
        );
        assert_eq!(
            get("watch_seen"),
            seen,
            "deferred changes must remain pending"
        );
        call(&mut life, "draft_change", doc["right"].clone())?;
        let before = get("included");
        call(&mut life, "toggle_include", path.clone())?;
        assert_eq!(
            get("doc"),
            UiValue::Map(doc.clone()),
            "staging must not navigate"
        );
        assert_eq!(
            get("busy"),
            UiValue::Bool(true),
            "checkbox must dispatch Git staging"
        );
        assert_eq!(
            get("included"),
            before,
            "checkbox waits for actual Git status"
        );
        call(
            &mut life,
            "failed",
            UiValue::String("simulated completion".into()),
        )?;
        call(
            &mut life,
            "toggle_group",
            UiValue::Array(vec![path.clone()]),
        )?;
        assert_eq!(get("busy"), UiValue::Bool(true));
        call(
            &mut life,
            "failed",
            UiValue::String("simulated completion".into()),
        )?;
        call(
            &mut life,
            "collapse_group",
            UiValue::String("Changes".into()),
        )?;
        call(
            &mut life,
            "collapse_group",
            UiValue::String("Changes".into()),
        )?;
        call(&mut life, "select", path.clone())?;
        assert_eq!(
            get("selection_id"),
            UiValue::Integer(0),
            "same-file click must be a no-op"
        );
        let tab = if doc["staged"] == UiValue::Bool(true) {
            "work"
        } else {
            "staged"
        };
        call(&mut life, "set_tab", UiValue::String(tab.into()))?;
        call(&mut life, "select", path)?;
        assert_eq!(get("busy"), UiValue::Bool(false));
        assert_eq!(get("selecting"), UiValue::Bool(true));
        assert_eq!(get("snapshot"), snapshot);
        let reply = |id| {
            UiValue::Map(BTreeMap::from([
                ("selection_id".into(), UiValue::Integer(id)),
                ("ok".into(), UiValue::Bool(true)),
                ("message".into(), UiValue::String("selected".into())),
                ("snapshot".into(), UiValue::Null),
                ("document".into(), UiValue::Map(doc.clone())),
            ]))
        };
        call(&mut life, "selected", reply(1))?;
        assert_eq!(
            get("selecting"),
            UiValue::Bool(true),
            "stale response must be ignored"
        );
        call(&mut life, "selected", reply(2))?;
        assert_eq!(get("selecting"), UiValue::Bool(false));
        assert_eq!(get("snapshot"), snapshot);
        call(
            &mut life,
            "draft_change",
            UiValue::String("unsaved tab draft".into()),
        )?;
        call(
            &mut life,
            "project_choose",
            UiValue::String("/another/project".into()),
        )?;
        assert_eq!(
            get("snapshot"),
            snapshot,
            "project switch must protect unsaved drafts"
        );
        assert_eq!(get("busy"), UiValue::Bool(false));
        call(&mut life, "close_tab", doc["path"].clone())?;
        assert_eq!(
            get("doc"),
            UiValue::Map(doc.clone()),
            "closing must preserve unsaved drafts"
        );
        call(&mut life, "closing", UiValue::Null)?;
        assert_eq!(get("confirm"), UiValue::String("close".into()));
        assert_eq!(get("draft"), UiValue::String("unsaved tab draft".into()));
        call(&mut life, "cancel", UiValue::Null)?;
        call(&mut life, "discard_now", UiValue::Null)?;
        call(&mut life, "close_tab", doc["path"].clone())?;
        assert_eq!(get("doc"), UiValue::Null);
        assert_eq!(get("open_tabs"), UiValue::Array(vec![]));
        call(
            &mut life,
            "commit_change",
            UiValue::String("first project message".into()),
        )?;
        call(
            &mut life,
            "filter_change",
            UiValue::String("first filter".into()),
        )?;
        let mut other = if let UiValue::Map(fields) = snapshot.clone() {
            fields
        } else {
            panic!("snapshot missing")
        };
        other.insert(
            "root".into(),
            UiValue::String("/alloy-test/other-project".into()),
        );
        let switch_result = |snapshot| {
            UiValue::Map(BTreeMap::from([
                ("ok".into(), UiValue::Bool(true)),
                ("message".into(), UiValue::String("switched".into())),
                ("snapshot".into(), snapshot),
                ("document".into(), UiValue::Null),
            ]))
        };
        call(&mut life, "loaded", switch_result(UiValue::Map(other)))?;
        assert_eq!(get("commit"), UiValue::String(String::new()));
        assert_eq!(get("filter"), UiValue::String(String::new()));
        assert!(matches!(get("included"), UiValue::Map(_)));
        call(
            &mut life,
            "commit_change",
            UiValue::String("second project message".into()),
        )?;
        call(&mut life, "loaded", switch_result(snapshot))?;
        assert_eq!(
            get("commit"),
            UiValue::String("first project message".into())
        );
        assert_eq!(get("filter"), UiValue::String("first filter".into()));

        life.render(&mut engine).map_err(error)?;
    }
    Ok(())
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
