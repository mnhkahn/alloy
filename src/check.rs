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
        // UI checks must never bootstrap the user's last real repository.
        fixture: Some(fixture.unwrap_or_else(|| {
            serde_json::json!({
                "ok":true,"message":"UI check","snapshot":null,"document":null
            })
        })),
        gate: Arc::new(Mutex::new(())),
    }
    .configure_runtime(&mut runtime)?;
    AppManifest::new(ModuleId::parse("main").unwrap())
        .with_capability("alloy.boot", "*")
        .map_err(error)?
        .with_capability("alloy.update", "*")
        .map_err(error)?
        .with_capability("alloy.awake", "*")
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
        ("acp_dropdown_change", UiValue::Null),
        (
            "acp_dropdown_change",
            UiValue::String("agent:Xiaoli".into()),
        ),
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
    // Keep-awake controls are independent of Git busy state, and status updates
    // must preserve the user's opt-in while showing active tasks and errors.
    assert_eq!(get("awake_enabled"), UiValue::Bool(false));
    let previous_busy = get("busy");
    runtime.borrow_mut().component_state.set(&state_path, "busy", UiValue::Bool(true)).map_err(error)?;
    let toggle = engine.callback(&compiled, "awake_toggle").map_err(error)?;
    for enabled in [true, false] {
        let _ = life.invoke_callback_transactional(&engine, &toggle, UiValue::Null).map_err(error)?;
        assert_eq!(get("awake_enabled"), UiValue::Bool(enabled));
        assert_eq!(get("busy"), UiValue::Bool(true));
        life.render(&mut engine).map_err(error)?;
    }
    runtime.borrow_mut().component_state.set(&state_path, "busy", previous_busy).map_err(error)?;
    let awake_update = engine.callback(&compiled, "awake_update").map_err(error)?;
    let _ = life.invoke_callback_transactional(&engine, &awake_update, UiValue::Map(BTreeMap::from([
        ("enabled".into(), UiValue::Bool(true)),
        ("active".into(), UiValue::Bool(true)),
        ("agents".into(), UiValue::Array([("codex:a", "Codex", "修复登录"), ("herdr:b", "opencode", "")].into_iter().map(|(id, name, title)| UiValue::Map(BTreeMap::from([
            ("id".into(), UiValue::String(id.into())), ("name".into(), UiValue::String(name.into())), ("title".into(), UiValue::String(title.into())),
            ("project".into(), UiValue::String("/same-project".into())), ("source".into(), UiValue::String("Local".into())),
        ]))).collect())),
        ("error".into(), UiValue::String("Herdr unavailable".into())),
    ]))).map_err(error)?;
    assert_eq!(get("awake_active"), UiValue::Bool(true));
    assert!(matches!(get("awake_agents"), UiValue::Array(agents) if agents.len()==2));
    let open = engine.callback(&compiled, "awake_open_changed").map_err(error)?;
    let _ = life.invoke_callback_transactional(&engine, &open, UiValue::Bool(true)).map_err(error)?;
    assert_eq!(get("awake_enabled"), UiValue::Bool(true), "opening the list must not toggle prevention");
    let tree=life.render(&mut engine).map_err(error)?;
    assert!(matches!(node_with_key(tree, "working-agent-count").map(UiNode::kind), Some(UiNodeKind::Text{text}) if text.as_str()=="2"));
    for id in ["codex:a", "herdr:b"] {assert!(node_with_key(tree, &format!("working-agent-{id}")).is_some());}
    assert_eq!(text_occurrences(tree, "opencode"), 1);
    assert_eq!(text_occurrences(tree, "OpenCode"), 0, "a missing task title must stay blank instead of repeating the executable name");
    assert_eq!(text_occurrences(tree, "修复登录"), 1);
    let list=node_with_key(tree,"awake-agent-list").unwrap();
    for label in ["Agent","标题","路径"] {assert_eq!(text_occurrences(list,label),2,"each agent must retain all three labeled fields");}
    assert_eq!(text_occurrences(tree, "Local"), 0, "source labels must not appear in the agent list");
    assert_eq!(text_occurrences(tree, "/same-project"), 2);
    assert!(matches!(get("awake_detail"), UiValue::String(detail) if detail.contains("Herdr unavailable")));
    life.render(&mut engine).map_err(error)?;
    let _ = life.invoke_callback_transactional(&engine, &toggle, UiValue::Null).map_err(error)?;
    let _ = life.invoke_callback_transactional(&engine, &open, UiValue::Bool(false)).map_err(error)?;
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
        if doc["conflict"] == UiValue::Bool(true) && doc["note"] == UiValue::String(String::new()) {
            let mut merge_snapshot = if let UiValue::Map(fields) = get("snapshot") {
                fields
            } else {
                unreachable!()
            };
            merge_snapshot.insert("merging".into(), UiValue::Bool(true));
            let update = UiValue::Map(BTreeMap::from([
                ("ok".into(), UiValue::Bool(true)),
                (
                    "message".into(),
                    UiValue::String("Merge in progress".into()),
                ),
                ("snapshot".into(), UiValue::Map(merge_snapshot)),
                ("document".into(), UiValue::Map(doc.clone())),
                (
                    "merge_message".into(),
                    UiValue::String("Merge incoming branch".into()),
                ),
            ]));
            let commit_change = engine.callback(&compiled, "commit_change").map_err(error)?;
            let loaded = engine.callback(&compiled, "loaded").map_err(error)?;
            let _ = life
                .invoke_callback_transactional(
                    &engine,
                    &commit_change,
                    UiValue::String("Earlier generated message".into()),
                )
                .map_err(error)?;
            let _ = life
                .invoke_callback_transactional(&engine, &loaded, update.clone())
                .map_err(error)?;
            assert_eq!(
                get("commit"),
                UiValue::String("Merge incoming branch".into())
            );
            let _ = life
                .invoke_callback_transactional(
                    &engine,
                    &commit_change,
                    UiValue::String("Reviewed merge message".into()),
                )
                .map_err(error)?;
            let _ = life
                .invoke_callback_transactional(&engine, &loaded, update)
                .map_err(error)?;
            assert_eq!(
                get("commit"),
                UiValue::String("Reviewed merge message".into())
            );
            life.render(&mut engine).map_err(error)?;
            for key in [
                "three-way-merge",
                "merge-local-pane",
                "merge-result-pane",
                "merge-incoming-pane",
            ] {
                assert!(
                    find_node(life.root().unwrap(), key).is_some(),
                    "Missing merge pane: {key}"
                );
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
        let call = |engine: &RuntimeEngine,
                    life: &mut ScriptLifecycle,
                    name: &str,
                    payload: UiValue|
         -> Result<(), String> {
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
        call(
            &engine,
            &mut life,
            "watch_loaded",
            response(UiValue::Integer(-1)),
        )?;
        assert_eq!(get("watch_seen"), seen);
        call(
            &engine,
            &mut life,
            "draft_change",
            UiValue::String("unsaved auto-refresh draft".into()),
        )?;
        call(
            &engine,
            &mut life,
            "watch_loaded",
            response(get("watch_epoch")),
        )?;
        assert_eq!(
            get("draft"),
            UiValue::String("unsaved auto-refresh draft".into())
        );
        assert_eq!(
            get("watch_seen"),
            seen,
            "deferred changes must remain pending"
        );
        // Other project summaries refresh even while a draft blocks the worktree watcher.
        let before_doc = get("doc");
        let before_message = get("message");
        let before_agent = get("selected_acp");
        let before_busy = get("busy");
        call(
            &engine,
            &mut life,
            "refresh_project_summaries",
            UiValue::Null,
        )?;
        assert_eq!(get("summaries_pending"), UiValue::Bool(true));
        assert_eq!(get("busy"), before_busy);
        let summary = UiValue::Map(BTreeMap::from([
            (
                "root".into(),
                UiValue::String("/alloy-test/unopened".into()),
            ),
            ("name".into(), UiValue::String("Unopened".into())),
            ("branch".into(), UiValue::String("main".into())),
            ("upstream".into(), UiValue::String("origin/main".into())),
            ("error".into(), UiValue::String(String::new())),
            ("files".into(), UiValue::Integer(3)),
            ("added".into(), UiValue::Integer(1)),
            ("modified".into(), UiValue::Integer(2)),
            ("deleted".into(), UiValue::Integer(0)),
            ("staged".into(), UiValue::Integer(0)),
            ("conflicts".into(), UiValue::Integer(0)),
            ("ahead".into(), UiValue::Integer(1)),
            ("behind".into(), UiValue::Integer(2)),
        ]));
        call(
            &engine,
            &mut life,
            "project_summaries_loaded",
            UiValue::Map(BTreeMap::from([(
                "projects".into(),
                UiValue::Array(vec![summary.clone()]),
            )])),
        )?;
        assert_eq!(get("summaries_pending"), UiValue::Bool(false));
        assert_eq!(get("doc"), before_doc);
        assert_eq!(
            get("draft"),
            UiValue::String("unsaved auto-refresh draft".into())
        );
        assert_eq!(get("message"), before_message);
        assert_eq!(get("selected_acp"), before_agent);
        if let UiValue::Array(projects) = get("projects") {
            assert!(projects.contains(&summary));
        } else {
            unreachable!()
        }
        call(&engine, &mut life, "draft_change", doc["right"].clone())?;
        let before = get("included");
        call(&engine, &mut life, "toggle_include", path.clone())?;
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
            &engine,
            &mut life,
            "failed",
            UiValue::String("simulated completion".into()),
        )?;
        call(
            &engine,
            &mut life,
            "toggle_group",
            UiValue::Array(vec![path.clone()]),
        )?;
        assert_eq!(get("busy"), UiValue::Bool(true));
        call(
            &engine,
            &mut life,
            "failed",
            UiValue::String("simulated completion".into()),
        )?;
        call(
            &engine,
            &mut life,
            "collapse_group",
            UiValue::String("Changes".into()),
        )?;
        call(
            &engine,
            &mut life,
            "collapse_group",
            UiValue::String("Changes".into()),
        )?;
        call(&engine, &mut life, "select", path.clone())?;
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
        call(&engine, &mut life, "set_tab", UiValue::String(tab.into()))?;
        call(&engine, &mut life, "select", path)?;
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
        call(&engine, &mut life, "selected", reply(1))?;
        assert_eq!(
            get("selecting"),
            UiValue::Bool(true),
            "stale response must be ignored"
        );
        call(&engine, &mut life, "selected", reply(2))?;
        assert_eq!(get("selecting"), UiValue::Bool(false));
        assert_eq!(get("snapshot"), snapshot);
        // AI commits keep write actions locked while file reads complete independently.
        for field in ["busy", "ai_running"] {
            runtime.borrow_mut().component_state
                .set(&state_path, field, UiValue::Bool(true)).map_err(error)?;
        }
        call(&engine, &mut life, "set_tab", UiValue::String(tab.into()))?;
        assert_eq!(get("selecting"), UiValue::Bool(true));
        assert_eq!(get("selection_id"), UiValue::Integer(3));
        call(&engine, &mut life, "selected", reply(3))?;
        assert_eq!(get("selecting"), UiValue::Bool(false));
        assert_eq!(get("busy"), UiValue::Bool(true), "diff reads must not unlock writes");
        assert_eq!(get("ai_running"), UiValue::Bool(true));
        call(&engine, &mut life, "stage", UiValue::Null)?;
        assert_eq!(get("busy"), UiValue::Bool(true));
        // A completion racing an outstanding diff invalidates that older response.
        call(&engine, &mut life, "set_tab", UiValue::String(tab.into()))?;
        let completion = UiValue::Map(BTreeMap::from([
            ("ok".into(), UiValue::Bool(true)),
            ("message".into(), UiValue::String("AI commit completed".into())),
            ("snapshot".into(), snapshot.clone()),
            ("document".into(), UiValue::Map(doc.clone())),
        ]));
        call(&engine, &mut life, "ai_finished", completion)?;
        assert_eq!(get("busy"), UiValue::Bool(false));
        assert_eq!(get("ai_running"), UiValue::Bool(false));
        assert_eq!(get("selecting"), UiValue::Bool(true));
        call(&engine, &mut life, "selected", reply(4))?;
        assert_eq!(get("selecting"), UiValue::Bool(true));
        call(&engine, &mut life, "selected", reply(6))?;
        assert_eq!(get("selecting"), UiValue::Bool(false));
        life.render(&mut engine).map_err(error)?;
        call(
            &engine,
            &mut life,
            "draft_change",
            UiValue::String("unsaved tab draft".into()),
        )?;
        call(
            &engine,
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
        call(&engine, &mut life, "close_tab", doc["path"].clone())?;
        assert_eq!(
            get("doc"),
            UiValue::Map(doc.clone()),
            "closing must preserve unsaved drafts"
        );
        call(&engine, &mut life, "closing", UiValue::Null)?;
        assert_eq!(get("confirm"), UiValue::String("close".into()));
        assert_eq!(get("draft"), UiValue::String("unsaved tab draft".into()));
        call(&engine, &mut life, "cancel", UiValue::Null)?;
        call(&engine, &mut life, "discard_now", UiValue::Null)?;
        call(&engine, &mut life, "close_tab", doc["path"].clone())?;
        assert_eq!(get("doc"), UiValue::Null);
        assert_eq!(get("open_tabs"), UiValue::Array(vec![]));
        call(
            &engine,
            &mut life,
            "commit_change",
            UiValue::String("first project message".into()),
        )?;
        call(
            &engine,
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
        call(
            &engine,
            &mut life,
            "loaded",
            switch_result(UiValue::Map(other)),
        )?;
        assert_eq!(get("commit"), UiValue::String(String::new()));
        assert_eq!(get("filter"), UiValue::String(String::new()));
        assert!(matches!(get("included"), UiValue::Map(_)));
        call(
            &engine,
            &mut life,
            "commit_change",
            UiValue::String("second project message".into()),
        )?;
        call(&engine, &mut life, "loaded", switch_result(snapshot))?;
        assert_eq!(
            get("commit"),
            UiValue::String("first project message".into())
        );
        assert_eq!(get("filter"), UiValue::String("first filter".into()));

        // History uses a separate document and only opens a diff on double-click.
        call(&engine, &mut life, "loaded", reply(2))?;
        let work_doc = get("doc");
        let work_draft = get("draft");
        call(&engine, &mut life, "history_toggle", UiValue::Null)?;
        assert_eq!(get("log_mode"), UiValue::Bool(true));
        let id = UiValue::String("a".repeat(40));
        let commit = UiValue::Map(BTreeMap::from([
            ("id".into(), id.clone()),
            ("parents".into(), UiValue::Array(vec![])),
            ("author".into(), UiValue::String("Test author".into())),
            (
                "date".into(),
                UiValue::String("2026-10-14T10:00:00+08:00".into()),
            ),
            ("subject".into(), UiValue::String("History subject".into())),
        ]));
        let response = |op: &str| {
            BTreeMap::from([
                ("ok".into(), UiValue::Bool(true)),
                ("history_op".into(), UiValue::String(op.into())),
                ("history_root".into(), root.clone()),
                ("history_id".into(), id.clone()),
            ])
        };
        let mut page = response("history");
        page.insert("history_skip".into(), UiValue::Integer(0));
        page.insert(
            "history".into(),
            UiValue::Map(BTreeMap::from([
                ("commits".into(), UiValue::Array(vec![commit.clone()])),
                ("has_more".into(), UiValue::Bool(true)),
            ])),
        );
        call(&engine, &mut life, "history_loaded", UiValue::Map(page))?;
        assert_eq!(get("history_selected"), id);
        let mut details = response("history_commit");
        details.insert(
            "details".into(),
            UiValue::Map(BTreeMap::from([
                ("commit".into(), commit),
                (
                    "message".into(),
                    UiValue::String("History subject\n\nComplete body".into()),
                ),
                (
                    "files".into(),
                    UiValue::Array(vec![UiValue::Map(BTreeMap::from([
                        ("path".into(), doc["path"].clone()),
                        ("old_path".into(), UiValue::String(String::new())),
                        ("status".into(), UiValue::String("A".into())),
                    ]))]),
                ),
            ])),
        );
        call(&engine, &mut life, "history_loaded", UiValue::Map(details))?;
        life.render(&mut engine).map_err(error)?;
        let file_key = if let UiValue::String(path) = &doc["path"] {
            format!("history-file-{path}")
        } else {
            unreachable!()
        };
        let pointer = find_node(life.root().unwrap(), &file_key)
            .unwrap()
            .handler("pointer_up")
            .and_then(UiEventHandler::as_script)
            .unwrap()
            .clone();
        let click = |count| {
            UiValue::Map(BTreeMap::from([
                ("button".into(), UiValue::String("left".into())),
                ("click_count".into(), UiValue::Integer(count)),
            ]))
        };
        let _ = life
            .invoke_callback_transactional(&engine, &pointer, click(1))
            .map_err(error)?;
        assert_eq!(get("busy"), UiValue::Bool(false));
        assert_eq!(get("history_doc"), UiValue::Null);
        let _ = life
            .invoke_callback_transactional(&engine, &pointer, click(2))
            .map_err(error)?;
        assert_eq!(get("busy"), UiValue::Bool(true));
        let mut diff = response("history_diff");
        let mut historical = doc.clone();
        historical.insert("editable".into(), UiValue::Bool(false));
        historical.insert("conflict".into(), UiValue::Bool(false));
        diff.insert("document".into(), UiValue::Map(historical));
        call(&engine, &mut life, "history_loaded", UiValue::Map(diff))?;
        life.render(&mut engine).map_err(error)?;
        assert!(find_node(life.root().unwrap(), "history-diff").is_some());
        assert_eq!(get("doc"), work_doc);
        assert_eq!(get("draft"), work_draft);
        call(&engine, &mut life, "history_toggle", UiValue::Null)?;
        assert_eq!(get("log_mode"), UiValue::Bool(false));
        assert_eq!(get("doc"), work_doc);
        assert_eq!(get("draft"), work_draft);
        // Push UI tests use a nonexistent root, so no real remote can be contacted.
        let real_snapshot = get("snapshot");
        let mut push_snapshot = if let UiValue::Map(fields) = real_snapshot.clone() {
            fields
        } else {
            unreachable!()
        };
        push_snapshot.insert(
            "root".into(),
            UiValue::String("/alloy-test/push-ui-only".into()),
        );
        let load_push_context = |snapshot| {
            UiValue::Map(BTreeMap::from([
                ("ok".into(), UiValue::Bool(true)),
                ("message".into(), UiValue::String("Push UI test".into())),
                ("snapshot".into(), snapshot),
                ("document".into(), work_doc.clone()),
            ]))
        };
        call(
            &engine,
            &mut life,
            "loaded",
            load_push_context(UiValue::Map(push_snapshot)),
        )?;
        call(
            &engine,
            &mut life,
            "push_prepared",
            UiValue::Map(BTreeMap::from([
                ("ok".into(), UiValue::Bool(true)),
                ("needs_upstream".into(), UiValue::Bool(true)),
            ])),
        )?;
        assert_eq!(get("confirm"), UiValue::String("push".into()));
        assert_eq!(get("busy"), UiValue::Bool(false));
        call(&engine, &mut life, "cancel", UiValue::Null)?;
        call(
            &engine,
            &mut life,
            "push_prepared",
            UiValue::Map(BTreeMap::from([
                ("ok".into(), UiValue::Bool(true)),
                ("needs_upstream".into(), UiValue::Bool(false)),
            ])),
        )?;
        assert_eq!(get("confirm"), UiValue::String(String::new()));
        assert_eq!(
            get("busy"),
            UiValue::Bool(true),
            "Configured upstream must dispatch push directly"
        );
        assert_eq!(get("doc"), work_doc);
        assert_eq!(get("draft"), work_draft);
        call(
            &engine,
            &mut life,
            "failed",
            UiValue::String("Simulated push completion".into()),
        )?;
        call(
            &engine,
            &mut life,
            "loaded",
            load_push_context(real_snapshot),
        )?;
        life.render(&mut engine).map_err(error)?;
    }
    // Completion must retain the full generated message in the read-only box,
    // including when the subsequent Git commit fails.
    for (ok, status_text, generated) in [
        (
            true,
            "Committed staged changes.",
            "fix(acp): 修复进程回收\n\n1. 避免生成结果丢失\n2. 保留完整正文",
        ),
        (
            false,
            "Commit failed.",
            "fix(acp): 保留失败时生成的信息\n\n1. 支持查看生成结果",
        ),
    ] {
        let callback = engine.callback(&compiled, "loaded").map_err(error)?;
        let _ = life
            .invoke_callback_transactional(
                &engine,
                &callback,
                UiValue::Map(BTreeMap::from([
                    ("ok".into(), UiValue::Bool(ok)),
                    ("message".into(), UiValue::String(status_text.into())),
                    ("snapshot".into(), UiValue::Null),
                    ("document".into(), UiValue::Null),
                    ("committed".into(), UiValue::Bool(ok)),
                    ("commit_message".into(), UiValue::String(generated.into())),
                ])),
            )
            .map_err(error)?;
        assert_eq!(get("commit"), UiValue::String(generated.into()));
        assert_eq!(get("message"), UiValue::String(status_text.into()));
        life.render(&mut engine).map_err(error)?;
    }
    Ok(())
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn find_node<'a>(node: &'a UiNode, key: &str) -> Option<&'a UiNode> {
    if node.key().is_some_and(|k| k.as_str() == key) {
        return Some(node);
    }
    match node.kind() {
        UiNodeKind::Box { children } | UiNodeKind::Fragment { children } => {
            children.iter().find_map(|n| find_node(n, key))
        }
        _ => None,
    }
}

fn node_with_key<'a>(node: &'a UiNode, key: &str) -> Option<&'a UiNode> {
    if node.key().is_some_and(|value| value.as_str()==key) {return Some(node);}
    match node.kind() {
        UiNodeKind::Box{children} | UiNodeKind::Fragment{children} => children.iter().find_map(|child| node_with_key(child,key)),
        UiNodeKind::Overlay{trigger,content,..} => node_with_key(trigger,key).or_else(|| node_with_key(content,key)),
        UiNodeKind::Layer{content,..} => node_with_key(content,key),
        _ => None,
    }
}

fn text_occurrences(node: &UiNode, value: &str) -> usize {
    match node.kind() {
        UiNodeKind::Text{text} => usize::from(text.as_str()==value),
        UiNodeKind::Box{children} | UiNodeKind::Fragment{children} => children.iter().map(|child| text_occurrences(child,value)).sum(),
        UiNodeKind::Overlay{trigger,content,..} => text_occurrences(trigger,value)+text_occurrences(content,value),
        UiNodeKind::Layer{content,..} => text_occurrences(content,value),
        _ => 0,
    }
}
