import "components/context_menu" as context_menu;
import "components/tooltip" as tooltip;
import "components/watch_timer" as watch_timer;
import "components/input" as input;
import "components/select" as select;
import "components/textarea" as textarea;
import "components/diff_viewer" as diff;
import "components/code_viewer" as code;

fn string_field(value) { #{ schema: #{ type: "string" }, "default": #{ type: "string", value: value } } }
fn bool_field(value) { #{ schema: #{ type: "bool" }, "default": #{ type: "bool", value: value } } }
fn state_schema() { #{ fields: #{
    watch_tick_id: #{schema:#{type:"integer"}, "default":#{type:"integer",value:0}},
    watch_epoch: #{schema:#{type:"integer"}, "default":#{type:"integer",value:0}},
    watch_seen: #{schema:#{type:"integer"}, "default":#{type:"integer",value:0}},
    watch_pending: bool_field(false), watch_root: string_field(""),
    awake_enabled: bool_field(false), awake_active: bool_field(false), awake_detail: string_field("勾选后，在 Codex 或 Herdr 工作时保持系统和屏幕唤醒，防止闲置熄屏后自动锁屏。"),
    awake_open: bool_field(false), awake_error: string_field(""),
    awake_agents: #{schema:#{type:"ui_value"}, "default":#{type:"null"}},
    root: string_field(""), url: string_field(""), filter: string_field(""), message: string_field("Opening Alloy…"), commit: string_field(""),
    draft: string_field(""), mode: string_field("split"), tab: string_field("work"), confirm: string_field(""),
    hide_unchanged: bool_field(true), mode_open: bool_field(false), whitespace_open: bool_field(false), whitespace: string_field("exact"),
    open_tabs: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    summaries_pending: bool_field(false), summaries_error: string_field(""),
    large_file_rows: #{schema:#{type:"ui_value"}, "default":#{type:"null"}},
    projects_open: bool_field(false), projects_loading: bool_field(false), add_project: bool_field(false),
    acp_page_open: bool_field(false), acp_select_open: bool_field(false),
    acp_agents: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    direct_models: #{schema:#{type:"ui_value"}, "default":#{type:"null"}},
    selected_model: string_field(""), registration_type: string_field("acp"),
    model_base_url: string_field(""), model_id: string_field(""), model_api_key: string_field(""),
    selected_acp: string_field("Xiaoli"), acp_new_name: string_field(""), acp_new_command: string_field(""), acp_clear_new_fields: bool_field(false), acp_message: string_field(""),
    projects: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    project_sessions: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    selecting: bool_field(false), pending_path: string_field(""), pending_staged: bool_field(false),
    selection_id: #{ schema: #{ type: "ui_value" }, "default": #{ type: "integer", value: 0 } },
    sidebar_visible: bool_field(true), palette: string_field(""), palette_query: string_field(""),
    focus_commit_id: #{ schema: #{ type: "ui_value" }, "default": #{ type: "integer", value: 0 } },
    focus_filter_id: #{ schema: #{ type: "ui_value" }, "default": #{ type: "integer", value: 0 } },
    focus_editor_id: #{ schema: #{ type: "ui_value" }, "default": #{ type: "integer", value: 0 } },
    shortcuts: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    advanced: bool_field(false), image_actual: bool_field(false),
    partial: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    included: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    collapsed: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    tag: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    tag_choice: string_field(""),
    log_mode: bool_field(false), history_more: bool_field(false), history_selected: string_field(""), history_file: string_field(""),
    history_commits: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    history_details: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    history_doc: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    file_menu: string_field(""), rollback_path: string_field(""), delete_path: string_field(""),
    committed_paths: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    operation_error: string_field(""),
    sensitive_review: #{schema:#{type:"ui_value"}, "default":#{type:"null"}},
    busy: bool_field(false), ai_running: bool_field(false), editor: bool_field(false), base_view: bool_field(false),
    snapshot: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    doc: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } }
} } }
fn dirty(ctx) { let d = ctx.get_state("doc"); d != () && ctx.get_state("draft") != d.right }
fn status(ctx, message) { ctx.set_state("message", message); }
fn failed(ctx, error) { ctx.set_state("busy", false); ctx.set_state("operation_error", `${error}`); status(ctx, `${error}`); }
fn dismiss_error(ctx,payload) {ctx.set_state("operation_error","");}
fn prepare_images(ctx, document) {
    if document == () || document.images == () { return document; }
    for side in ["left", "right"] {
        let preview = document.images[side];
        if preview != () && preview.error == "" {
            try { ctx.load_image(asset(preview.asset)); }
            catch (error) { document.images[side].error = `${error}`; }
        }
    }
    document
}
// Compare staged file membership and full/partial inclusion independently of file order.
fn commit_selection(snapshot) {
    let selected = #{};
    if snapshot != () {for file in snapshot.files {
        if file.index != " " && file.index != "?" && !file.conflict {
            selected[file.path] = #{index:file.index,old_path:file.old_path,full:file.work == " "};
        }
    }}
    selected
}
fn loaded(ctx, result) {
    let prior=ctx.get_state("snapshot");
    let entering_merge=result.snapshot != () && result.snapshot.merging && (prior==() || !prior.merging);
    if "document" in result { result.document = prepare_images(ctx, result.document); }
    if !ctx.get_state("ai_running") {ctx.set_state("busy", false);}
    status(ctx, result.message);
    if !result.ok {ctx.set_state("operation_error",result.message);}
    if "sensitive_review" in result {ctx.set_state("sensitive_review",result.sensitive_review);ctx.set_state("confirm","sensitive-commit");}
    if "tag_retry" in result { ctx.set_state("tag",result.tag_retry); ctx.set_state("confirm","tag-retry"); }
    if "overview" in result { let projects = project_list(ctx); let found = false; for i in 0..projects.len() { if projects[i].root == result.overview.root { projects[i] = result.overview; found = true; } } if !found { projects.push(result.overview); } ctx.set_state("projects",projects); }
    if result.snapshot != () {
        ctx.set_state("file_menu", "");
        if "committed_paths" in result { ctx.set_state("committed_paths",result.committed_paths); }
        let previous_snapshot = ctx.get_state("snapshot");
        let switched = previous_snapshot == () || previous_snapshot.root != result.snapshot.root;
        if switched {
            ctx.set_state("log_mode",false);ctx.set_state("history_commits",[]);ctx.set_state("history_details",());ctx.set_state("history_doc",());ctx.set_state("history_file","");ctx.set_state("history_selected","");ctx.set_state("history_more",false);
            let sessions = ctx.get_state("project_sessions"); if sessions == () { sessions = #{}; }
            if previous_snapshot != () { sessions[previous_snapshot.root] = #{commit:ctx.get_state("commit"),filter:ctx.get_state("filter"),selection:commit_selection(previous_snapshot)}; }
            let saved = if result.snapshot.root in sessions {sessions[result.snapshot.root]} else {#{commit:"",filter:"",selection:#{}}};
            ctx.set_state("commit",if result.snapshot.merging || saved.selection == commit_selection(result.snapshot) {saved.commit} else {""}); ctx.set_state("filter",saved.filter); ctx.set_state("project_sessions",sessions);
            ctx.set_state("included",#{}); ctx.set_state("collapsed",#{}); ctx.set_state("editor",false); ctx.set_state("base_view",false);
        }
        if !switched && !result.snapshot.merging && commit_selection(previous_snapshot) != commit_selection(result.snapshot) {
            ctx.set_state("commit", "");
        }
        let current = #{}; let mixed = #{};
        if switched {ctx.set_state("open_tabs",[]);}
        for file in result.snapshot.files {
            let staged = file.index != " " && file.index != "?" && !file.conflict;
            current[file.path] = staged && file.work == " ";
            if staged && file.work != " " {mixed[file.path]=true;}
        }
        ctx.set_state("partial",mixed); ctx.set_state("included",current); ctx.set_state("snapshot",result.snapshot);sync_file_rows(ctx);
    }
    if "commit_message" in result && result.commit_message != () {ctx.set_state("commit",result.commit_message);}
    if "merge_message" in result && result.merge_message != () && (entering_merge || ctx.get_state("commit")=="") {ctx.set_state("commit",result.merge_message);}
    if result.snapshot != () && result.snapshot.merging && conflict_count(result.snapshot)>0 {ctx.set_state("log_mode",false);ctx.set_state("sidebar_visible",true);}
    if !result.ok {
        if !dirty(ctx) && result.document != () {
            ctx.set_state("doc", result.document); ctx.set_state("draft", result.document.right);
            ctx.set_state("editor", result.document.conflict); ctx.set_state("tab", "work");
        }
        return;
    }
    if "draft" in result { ctx.set_state("draft", result.draft); return; }
    let previous = ctx.get_state("doc");
    ctx.set_state("doc", result.document);
    if result.document != () { let tabs = opened(ctx); if !tabs.contains(result.document.path) { tabs.push(result.document.path); ctx.set_state("open_tabs", tabs); } }
    ctx.set_state("confirm", "");
    if result.ok && result.snapshot != () { ctx.set_state("add_project",false); }
    if result.snapshot != () { ctx.set_state("root", result.snapshot.root); }
    if result.document != () { ctx.set_state("draft", result.document.right); ctx.set_state("tab", if result.document.staged {"staged"} else {"work"}); if previous == () || previous.path != result.document.path { ctx.set_state("editor", result.document.conflict); } }
    else { ctx.set_state("draft", ""); }
    if "tag_preview" in result { ctx.set_state("tag",result.tag_preview); ctx.set_state("confirm","tag"); }
    if "tagged" in result { ctx.set_state("tag",()); ctx.set_state("tag_choice",""); }
}
fn request(ctx, op, extra) {
    watch_invalidate(ctx);
    if ctx.get_state("selecting") { return; }
    if ctx.get_state("busy") { return; }
    let d = ctx.get_state("doc");
    let active = ctx.get_state("snapshot");
    let repo_root = if op == "open" || op == "clone" || active == () { ctx.get_state("root") } else { active.root };
    let data = #{ op: op, root: repo_root, path: if d == () { "" } else { d.path }, staged: ctx.get_state("tab") == "staged" };
    for key in extra.keys() { data[key] = extra[key]; }
    // Clear immediately when a staging change starts, including changing blocks in
    // an already selected file; waiting for Git's reply leaves stale text visible.
    if active != () && !active.merging && ["stage_paths","stage_hunks","stage","unstage"].contains(op) {
        ctx.set_state("commit", "");
    }
    ctx.set_state("operation_error","");ctx.set_state("busy", true); sync_update_guard(ctx);status(ctx, `${op}…`);
    if op == "ai_commit" {
        ctx.set_state("ai_running",true);
        ctx.start_task("alloy.git", "execute", data, Fn("ai_finished"), Fn("ai_failed"));
    } else {ctx.start_task("alloy.git", "execute", data, Fn("loaded"), Fn("failed"));}
}
fn navigation_blocked(ctx) {ctx.get_state("busy") && !ctx.get_state("ai_running")}
fn ai_failed(ctx,error) {ctx.set_state("ai_running",false);failed(ctx,error);}
fn ai_finished(ctx,result) {
    let current=ctx.get_state("doc");
    let path=if ctx.get_state("selecting") {ctx.get_state("pending_path")} else if current!=() {current.path} else {""};
    let staged=if ctx.get_state("selecting") {ctx.get_state("pending_staged")} else {ctx.get_state("tab")=="staged"};
    let keep=false;
    if result.snapshot!=() {for file in result.snapshot.files {if file.path==path {keep=true;}}}
    ctx.set_state("selection_id",ctx.get_state("selection_id")+1);
    ctx.set_state("selecting",false);ctx.set_state("pending_path","");
    ctx.set_state("ai_running",false);
    if keep {result.document=current;}
    loaded(ctx,result);
    if keep && !("sensitive_review" in result) {load_selection(ctx,path,staged);}
}
fn sync_update_guard(ctx) {let s=ctx.get_state("snapshot");ctx.call_capability("alloy.update","guard",#{blocked:watch_blocked(ctx) || (s!=() && s.merging)});}
fn watch_invalidate(ctx) {ctx.set_state("watch_epoch",ctx.get_state("watch_epoch")+1);}
fn watch_blocked(ctx) {ctx.get_state("busy") || ctx.get_state("selecting") || dirty(ctx) || ctx.get_state("confirm") != ""}
fn watch_tick(ctx,payload) {
 ctx.set_state("watch_tick_id",ctx.get_state("watch_tick_id")+1);
 awake_update(ctx,ctx.call_capability("alloy.awake","status",()));
 sync_update_guard(ctx);
 if ctx.get_state("watch_tick_id") % 12 == 1 {refresh_project_summaries(ctx,());}
 if ctx.get_state("acp_page_open") {return;}
 let s=ctx.get_state("snapshot");
 if s == () || ctx.get_state("watch_pending") || watch_blocked(ctx) {return;}
 if ctx.get_state("watch_root") != s.root {ctx.set_state("watch_root",s.root);ctx.set_state("watch_seen",0);}
 let d=ctx.get_state("doc");
 ctx.set_state("watch_pending",true);
 ctx.start_task("alloy.git","execute",#{op:"watch",root:s.root,path:if d==() {""} else {d.path},staged:ctx.get_state("tab")=="staged",seen:ctx.get_state("watch_seen"),epoch:ctx.get_state("watch_epoch")},Fn("watch_loaded"),Fn("watch_failed"));
}
fn watch_failed(ctx,error) {ctx.set_state("watch_pending",false);status(ctx,`Auto-refresh: ${error}`);}
fn watch_loaded(ctx,result) {
 ctx.set_state("watch_pending",false);
 let s=ctx.get_state("snapshot");
 if s==() || s.root!=result.watch_root || ctx.get_state("watch_epoch")!=result.watch_epoch || watch_blocked(ctx) {return;}
 if result.revision==() {return;}
 ctx.set_state("watch_seen",result.revision);
 let update=result.update;
 if update==() {return;}
 if ctx.get_state("doc")==() {update.document=();}
 if s==update.snapshot && ctx.get_state("doc")==update.document {return;}
 let message=ctx.get_state("message"); let editing=ctx.get_state("editor"); let tab=ctx.get_state("tab");
 loaded(ctx,update);
 ctx.set_state("message",message);ctx.set_state("editor",editing);ctx.set_state("tab",tab);
}
fn guard(ctx) {
    if ctx.get_state("selecting") { return false; }
    if ctx.get_state("busy") { return false; }
    if dirty(ctx) { status(ctx, "You have an unsaved draft. Save it or discard it before changing context."); return false; }
    true
}
fn init(ctx) {
    let shortcuts = ctx.call_capability("alloy.boot","shortcuts",()); ctx.set_state("shortcuts",shortcuts);
    let registered = []; for shortcut in shortcuts { if !registered.contains(shortcut.action) {ctx.register_action(shortcut.action,Fn(shortcut.handler)); registered.push(shortcut.action);} }
    ctx.set_close_handler(Fn("closing"));
    ctx.set_state("projects_loading",true);ctx.start_task("alloy.git","execute",#{op:"projects"},Fn("projects_loaded"),Fn("projects_failed"));
    let fixture = ctx.call_capability("alloy.boot", "state", ());
    if fixture == () { request(ctx, "bootstrap", #{}); } else { loaded(ctx, fixture); }
}
fn close_active(ctx, payload) { let d = ctx.get_state("doc"); if d != () {close_tab(ctx,d.path);} }
fn move_tab(ctx, direction) {
    if !guard(ctx) {return;} let tabs = opened(ctx); let d = ctx.get_state("doc");
    if d == () || tabs.len() < 2 {return;}
    let i = tabs.index_of(d.path); select(ctx,tabs[(i + direction + tabs.len()) % tabs.len()]);
}
fn next_tab(ctx,payload) {move_tab(ctx,1);}
fn previous_tab(ctx,payload) {move_tab(ctx,-1);}
fn focus_commit(ctx,payload) { if ctx.get_state("snapshot") != () {ctx.set_state("sidebar_visible",true); ctx.set_state("focus_commit_id",ctx.get_state("focus_commit_id")+1);} }
fn focus_files(ctx,payload) {ctx.set_state("sidebar_visible",true); ctx.set_state("focus_filter_id",ctx.get_state("focus_filter_id")+1);}
fn toggle_sidebar(ctx,payload) {ctx.set_state("sidebar_visible",!ctx.get_state("sidebar_visible"));}
fn hide_sidebar(ctx,payload) {ctx.set_state("sidebar_visible",false); focus_diff(ctx,());}
fn focus_diff(ctx,payload) { let d=ctx.get_state("doc"); if d != () && d.images == () && d.note == "" { try {ctx.focus("active-diff");} catch (ignored) {} } }
fn edit_focus(ctx,payload) {let d=ctx.get_state("doc"); if d != () && d.editable && !ctx.get_state("busy") && !ctx.get_state("selecting") {ctx.set_state("editor",true);ctx.set_state("focus_editor_id",ctx.get_state("focus_editor_id")+1);} }
fn diff_settings(ctx,payload) { let d=ctx.get_state("doc"); if d != () && d.images == () {ctx.set_state("mode_open",true);} }
fn escape_action(ctx,payload) {ctx.set_state("palette","");ctx.set_state("confirm","");ctx.set_state("projects_open",false);ctx.set_state("mode_open",false);ctx.set_state("whitespace_open",false);ctx.set_state("add_project",false);focus_diff(ctx,());}
fn push_prompt(ctx,payload) {
    if !guard(ctx) || ctx.get_state("snapshot")==() {return;}
    watch_invalidate(ctx);ctx.set_state("confirm","");ctx.set_state("busy",true);sync_update_guard(ctx);status(ctx,"Push…");
    ctx.start_task("alloy.git","execute",#{op:"push_prepare",root:ctx.get_state("snapshot").root},Fn("push_prepared"),Fn("failed"));
}
fn push_prepared(ctx,result) {
    ctx.set_state("busy",false);
    if !result.ok {status(ctx,result.message);return;}
    if result.needs_upstream {ctx.set_state("confirm","push");status(ctx,"No upstream configured · confirm the first push");}
    else {request(ctx,"push",#{allow_upstream:false});}
}
fn pull_prompt(ctx,payload) {if guard(ctx) && ctx.get_state("snapshot") != () {ctx.set_state("confirm","pull");}}
fn network_confirm(ctx,payload) {if !guard(ctx) {return;}let op=ctx.get_state("confirm");if op == "push" {ctx.set_state("confirm","");request(ctx,"push",#{allow_upstream:true});} else if op == "pull" {ctx.set_state("confirm","");network(ctx,op);}}
fn commit_shortcut(ctx,payload) {if ctx.get_state("snapshot") != () && has_message(ctx) && selected_count(ctx)>0 {commit(ctx,());} else {status(ctx,"Stage changes and enter a commit message first.");}}
fn sensitive_commit_now(ctx,payload) {
    let review=ctx.get_state("sensitive_review"); let s=ctx.get_state("snapshot");
    if review==() || s==() || review.root!=s.root {return;}
    if guard(ctx) {ctx.set_state("confirm","");ctx.set_state("sensitive_review",());request(ctx,"ai_commit",#{confirmed_tree:review.tree});}
}
fn ai_commit(ctx,payload) {if guard(ctx) && ctx.get_state("snapshot") != () {request(ctx,"ai_commit",#{});}}
fn palette_open(ctx,mode) {ctx.set_state("palette_query","");ctx.set_state("palette",mode);}
fn find_action(ctx,payload) {palette_open(ctx,"actions");}
fn vcs_actions(ctx,payload) {palette_open(ctx,"vcs");}
fn recent_files(ctx,payload) {palette_open(ctx,"recent");}
fn find_file(ctx,payload) {palette_open(ctx,"files");}
fn palette_query(ctx,value) {ctx.set_state("palette_query",value);}
fn palette_items(ctx) {
    let result=[]; let mode=ctx.get_state("palette"); let q=ctx.get_state("palette_query").to_lower();
    if mode == "actions" || mode == "vcs" {
        let seen=[]; for item in ctx.get_state("shortcuts") {
            if !seen.contains(item.action) && (mode != "vcs" || item.group == "VCS") && (q == "" || item.label.to_lower().contains(q) || item.key.contains(q)) { result.push(#{label:item.label,hint:"",value:item.action}); seen.push(item.action); }
        }
    } else {
        let paths=opened(ctx); let s=ctx.get_state("snapshot");
        if mode == "files" { paths=[]; if s != () {for f in s.files {paths.push(f.path);}} }
        for path in paths {if q == "" || path.to_lower().contains(q) {result.push(#{label:file_name(path),hint:path,value:path});}}
    }
    result
}
fn palette_choose(ctx,value) {
    let mode=ctx.get_state("palette");ctx.set_state("palette","");
    if mode == "actions" || mode == "vcs" {ctx.dispatch_action(value,());} else if ctx.get_state("snapshot") != () {select(ctx,value);}
}
fn palette_submit(ctx,payload) {let items=palette_items(ctx);if items.len()>0 {palette_choose(ctx,items[0].value);}}
fn palette_view(ctx) {
    let mode=ctx.get_state("palette");if mode == "" {return row([]);}
    let nodes=[row([text(if mode == "recent" {"Recent / open files"} else if mode == "files" {"Go to changed file"} else {"Find action"}),button(ctx,"Close",Fn("escape_action"),(),true)]).with_style(style().justify_between()),input::Input(#{key:`palette-${mode}`,label:"Search",value:ctx.get_state("palette_query"),placeholder:"Type to filter",autofocus:true,on_change:Fn("palette_query"),on_submit:Fn("palette_submit")})];
    for item in palette_items(ctx) {nodes.push(row([text(item.label),small(item.hint)]).with_key(item.value).with_style(style().padding(px(5)).justify_between().hover(style().background(theme_color("selection")))).tab_index(0).on_key_value("enter",Fn("palette_choose"),item.value).on_click_value(Fn("palette_choose"),item.value));}
    column(nodes).with_key("action-palette").with_style(style().padding(px(10)).gap(px(4)).max_height(px(300)).overflow_y_scroll().flex_shrink(false).background(theme_color("surface_raised")))
}
fn closing(ctx, payload) {
    if dirty(ctx) || ctx.get_state("busy") { ctx.set_state("confirm", "close"); }
    else { ctx.close_window(ctx.window_id()); }
}
fn close_now(ctx, payload) { ctx.close_window(ctx.window_id()); }
fn cancel(ctx, payload) { ctx.set_state("sensitive_review",()); ctx.set_state("confirm", ""); ctx.set_state("tag", ()); ctx.set_state("tag_choice", ""); }
fn project_list(ctx) { let value = ctx.get_state("projects"); if value == () {[]} else {value} }
fn refresh_project_summaries(ctx,payload) {
    if ctx.get_state("summaries_pending") || ctx.get_state("projects_loading") {return;}
    ctx.set_state("summaries_pending",true);
    ctx.start_task("alloy.git","execute",#{op:"project_summaries"},Fn("project_summaries_loaded"),Fn("project_summaries_failed"));
}
fn project_summaries_loaded(ctx,result) {
    ctx.set_state("summaries_pending",false);ctx.set_state("summaries_error","");
    // Keep the active repository's newer, document-associated snapshot.
    let s=ctx.get_state("snapshot");let current=();
    if s!=() {for project in project_list(ctx) {if project.root==s.root {current=project;}}}
    let projects=[];let found=false;
    for project in result.projects {if current!=() && project.root==current.root {projects.push(current);found=true;} else {projects.push(project);}}
    if current!=() && !found {projects.push(current);}
    ctx.set_state("projects",projects);
}
fn project_summaries_failed(ctx,error) {ctx.set_state("summaries_pending",false);ctx.set_state("summaries_error",`Project summaries: ${error}`);}
fn projects_loaded(ctx, result) {
    ctx.set_state("projects_loading",false); ctx.set_state("projects",result.projects);
    if "acp_agents" in result {apply_ai_settings(ctx,result);}
}
fn projects_failed(ctx, error) { ctx.set_state("projects_loading",false); if ctx.get_state("acp_page_open") {acp_status(ctx,`无法加载 ACP 配置：${error}`);} else {status(ctx,`Cannot refresh projects: ${error}`);} }
fn projects_toggle(ctx, value) {
    ctx.set_state("projects_open",value);
    // The cached list opens immediately; the timer owns background refreshes.
}
fn acp_page_open(ctx, payload) {
    ctx.set_state("acp_page_open",true);
    ctx.set_state("acp_message","");
    ctx.set_state("projects_open",false);
    if !ctx.get_state("projects_loading") {
        ctx.set_state("projects_loading",true); ctx.start_task("alloy.git","execute",#{op:"projects"},Fn("projects_loaded"),Fn("projects_failed"));
    }
}
fn acp_page_close(ctx, payload) {ctx.set_state("model_api_key","");ctx.set_state("acp_page_open",false);}
fn acp_agent_list(ctx) {let agents=ctx.get_state("acp_agents");if agents==() {[]} else {agents}}
fn direct_model_list(ctx) {let models=ctx.get_state("direct_models");if models==() {[]} else {models}}
fn apply_ai_settings(ctx,result) {
    ctx.set_state("acp_agents",result.acp_agents);ctx.set_state("selected_acp",result.selected_acp);
    if "direct_models" in result {ctx.set_state("direct_models",result.direct_models);ctx.set_state("selected_model",result.selected_model);}
}
fn acp_agent_command(agent) {if "command" in agent && agent.command!="" {agent.command} else {`${agent.path} acp`}}
fn acp_options(ctx) {
    let options=[];
    for agent in acp_agent_list(ctx) {options.push(#{value:`agent:${agent.name}`,label:`ACP · ${agent.name}`});}
    for model in direct_model_list(ctx) {options.push(#{value:`model:${model.name}`,label:`模型 · ${model.name}`});}
    options.push(#{value:"register",label:"手动注册"});options
}
fn acp_agent_selector(ctx) {
    let selected=if ctx.get_state("selected_model")!="" {`model:${ctx.get_state("selected_model")}`} else {`agent:${ctx.get_state("selected_acp")}`};
    let options=acp_options(ctx);let found=false;for option in options {if option.value==selected {found=true;}}
    if !found {selected="register";}
    select::Select(#{key:"acp-agent-selector",label:"AI provider",placement:"top",disabled:ctx.get_state("busy"),options:options,value:selected,open:ctx.get_state("acp_select_open"),query:"",empty_text:"No providers",size:"xs",width:px(176),on_change:Fn("acp_dropdown_change"),on_open_change:Fn("acp_select_open_changed")})
}
fn acp_dropdown_change(ctx,value) {
    if value==() {return;}
    ctx.set_state("acp_select_open",false);
    if value=="register" {acp_page_open(ctx,());return;}
    if value.starts_with("model:") {model_settings_request(ctx,"select_model",#{name:value.sub_string(6)});return;}
    if value.starts_with("agent:") {acp_select_change(ctx,value.sub_string(6));}
}
fn acp_select_open_changed(ctx,value) {ctx.set_state("acp_select_open",value);}
fn acp_status(ctx,message) {ctx.set_state("acp_message",message);status(ctx,message);}
fn acp_settings_saved(ctx,result) {
    if !result.ok {acp_settings_failed(ctx,result.message);return;}
    ctx.set_state("busy",false);apply_ai_settings(ctx,result);
    acp_status(ctx,if ctx.get_state("acp_clear_new_fields") {"已注册并设为当前 AI 提供方。"} else {"AI 配置已保存。"});
    if ctx.get_state("acp_clear_new_fields") {
        ctx.set_state("acp_new_name","");ctx.set_state("acp_new_command","");
        ctx.set_state("model_base_url","");ctx.set_state("model_id","");ctx.set_state("model_api_key","");
        ctx.set_state("acp_clear_new_fields",false);
    }
}
fn acp_settings_failed(ctx,error) {ctx.set_state("busy",false);ctx.set_state("acp_clear_new_fields",false);acp_status(ctx,`保存失败：${error}`);}
fn save_acp_settings(ctx,agents,selected,activate) {
    if ctx.get_state("busy") {return;}
    ctx.set_state("busy",true);sync_update_guard(ctx);acp_status(ctx,"正在保存 ACP 配置…");
    ctx.start_task("alloy.git","execute",#{op:"save_acp_settings",acp_agents:agents,selected_acp:selected,activate_acp:activate},Fn("acp_settings_saved"),Fn("acp_settings_failed"));
}
fn acp_select_change(ctx,value) {save_acp_settings(ctx,acp_agent_list(ctx),value,true);}
fn acp_new_name_change(ctx,value) {ctx.set_state("acp_new_name",value);}
fn acp_new_command_change(ctx,value) {ctx.set_state("acp_new_command",value);}
fn acp_add_agent(ctx,payload) {
    if ctx.get_state("busy") || ctx.get_state("projects_loading") {return;}
    let name=ctx.get_state("acp_new_name");name.trim();
    let command=ctx.get_state("acp_new_command");command.trim();
    if name=="" || command=="" {acp_status(ctx,"请填写名称和命令字符串。");return;}
    let agents=acp_agent_list(ctx);
    for agent in agents {if agent.name==name {acp_status(ctx,`名称 ${name} 已存在，请使用其他名称。`);return;}}
    agents.push(#{name:name,path:"",command:command});ctx.set_state("acp_clear_new_fields",true);
    save_acp_settings(ctx,agents,name,true);
}
fn registration_type_change(ctx,value) {ctx.set_state("registration_type",value);ctx.set_state("model_api_key","");}
fn model_base_url_change(ctx,value) {ctx.set_state("model_base_url",value);}
fn model_id_change(ctx,value) {ctx.set_state("model_id",value);}
fn model_api_key_change(ctx,value) {ctx.set_state("model_api_key",value);}
fn model_settings_request(ctx,op,extra) {
    if ctx.get_state("busy") || ctx.get_state("projects_loading") {return;}
    let data=#{op:op};for key in extra.keys() {data[key]=extra[key];}
    ctx.set_state("busy",true);sync_update_guard(ctx);acp_status(ctx,"正在保存模型配置…");
    ctx.start_task("alloy.git","execute",data,Fn("acp_settings_saved"),Fn("acp_settings_failed"));
}
fn model_register(ctx,payload) {
    if ctx.get_state("busy") || ctx.get_state("projects_loading") {return;}
    let name=ctx.get_state("acp_new_name");name.trim();
    let base=ctx.get_state("model_base_url");base.trim();let model=ctx.get_state("model_id");model.trim();
    if name=="" || base=="" || model=="" {acp_status(ctx,"请填写名称、Base URL 和模型名。");return;}
    ctx.set_state("acp_clear_new_fields",true);
    model_settings_request(ctx,"register_model",#{model:#{name:name,base_url:base,model:model,api_key:ctx.get_state("model_api_key"),max_tokens:4096}});
}
fn model_choose(ctx,name) {model_settings_request(ctx,"select_model",#{name:name});}
fn model_remove(ctx,name) {model_settings_request(ctx,"remove_model",#{name:name});}
fn model_edit(ctx,name) {
    for model in direct_model_list(ctx) {if model.name==name {
        ctx.set_state("registration_type","model");ctx.set_state("acp_new_name",name);
        ctx.set_state("model_base_url",model.base_url);ctx.set_state("model_id",model.model);ctx.set_state("model_api_key","");
        acp_status(ctx,"编辑后点注册 / 更新。API Key 留空保留已有密钥。");return;
    }}
}
fn acp_remove_agent(ctx,name) {
    let agents=acp_agent_list(ctx);if agents.len()<2 {acp_status(ctx,"至少保留一个 ACP 代理。");return;}
    let remaining=[];for agent in agents {if agent.name!=name {remaining.push(agent);}}
    let selected=ctx.get_state("selected_acp");if selected==name {selected=remaining[0].name;}
    save_acp_settings(ctx,remaining,selected,false);
}
fn project_pointer(root,ctx,event) {
    if event.button=="left" {project_choose(ctx,root);}
}
fn project_choose(ctx, root) {
    if !guard(ctx) { return; }
    let s = ctx.get_state("snapshot");
    ctx.set_state("projects_open",false);
    if s != () && s.root == root { return; }
    request(ctx,"open",#{root:root,staged:false,path:""});
}
fn project_add(ctx, payload) { if guard(ctx) { ctx.set_state("projects_open",false); ctx.set_state("add_project",!ctx.get_state("add_project")); } }
fn root_change(ctx, value) { ctx.set_state("root", value); }
fn url_change(ctx, value) { ctx.set_state("url", value); }
fn filter_change(ctx, value) { ctx.set_state("filter", value);sync_file_rows(ctx); }
fn commit_change(ctx, value) { ctx.set_state("commit", value); }
fn draft_change(ctx, value) { watch_invalidate(ctx); if !ctx.get_state("selecting") { ctx.set_state("draft", value); } sync_update_guard(ctx); }
fn open_repo(ctx, payload) { if guard(ctx) { request(ctx, "open", #{}); } }
fn browse(ctx, payload) { if guard(ctx) { request(ctx, "browse", #{}); } }
fn clone_repo(ctx, payload) { if guard(ctx) { request(ctx, "clone", #{url:ctx.get_state("url")}); } }
fn refresh(ctx, payload) { if ctx.get_state("log_mode") {if guard(ctx) {history_request(ctx,"history",#{skip:0});} return;} if guard(ctx) && ctx.get_state("snapshot") != () { request(ctx, "refresh", #{default_selection:true}); } }
fn file_menu_open(path, ctx, open) { ctx.set_state("file_menu",if open {path} else {""}); }
fn can_rollback(ctx, path) { let paths=ctx.get_state("committed_paths"); if paths==() {return false;} for file in ctx.get_state("snapshot").files {if file.path==path {return paths.contains(path) || paths.contains(file.old_path);}} false }
fn file_action(path, ctx, action) {
    ctx.set_state("file_menu", "");
    if action == "open-file" || action == "open-folder" {
        ctx.start_task("alloy.git","execute",#{op:if action=="open-file" {"open_file"} else {"open_folder"},root:ctx.get_state("snapshot").root,path:path},Fn("file_opened"),Fn("file_open_failed"));
        return;
    }
    if action == "add-to-gitignore" && guard(ctx) && !ctx.get_state("snapshot").merging {request(ctx,"add_to_gitignore",#{path:path,staged:false});}
    if action == "rollback" && guard(ctx) && can_rollback(ctx,path) {ctx.set_state("rollback_path",path);ctx.set_state("confirm","rollback");}
    if action == "delete-file" && guard(ctx) && !ctx.get_state("snapshot").merging {ctx.set_state("delete_path",path);ctx.set_state("confirm","delete-file");}
}
fn file_opened(ctx, result) {status(ctx,result.message);}
fn file_open_failed(ctx, error) {status(ctx,`${error}`);}
fn delete_file_now(ctx, payload) {
    if guard(ctx) {request(ctx,"delete_file",#{path:ctx.get_state("delete_path"),staged:false});}
}
fn rollback_all(ctx, payload) { if guard(ctx) {ctx.set_state("confirm","rollback-all");} }
fn rollback_now(ctx, payload) {
    if !guard(ctx) {return;}
    if ctx.get_state("confirm")=="rollback-all" {request(ctx,"rollback_all",#{});}
    else {request(ctx,"rollback",#{path:ctx.get_state("rollback_path")});}
}
fn selection_failed(ctx, error) {
    ctx.set_state("selecting", false); ctx.set_state("pending_path", ""); status(ctx, `${error}`);
}
fn selected(ctx, result) {
    if result.selection_id != ctx.get_state("selection_id") { return; }
    ctx.set_state("selecting", false); ctx.set_state("pending_path", "");
    if result.ok { loaded(ctx, result); ctx.set_state("tab", if result.document.staged {"staged"} else {"work"}); }
    else { status(ctx, result.message); }
}
fn select_file(ctx, path, staged) {
    watch_invalidate(ctx);
    if navigation_blocked(ctx) { return; }
    if dirty(ctx) { status(ctx, "You have an unsaved draft. Save it or discard it before changing context."); return; }
    let d = ctx.get_state("doc");
    if !ctx.get_state("selecting") && d != () && d.path == path && d.staged == staged { return; }
    load_selection(ctx,path,staged);
}
fn load_selection(ctx,path,staged) {
    let old_path = "";
    for file in ctx.get_state("snapshot").files { if file.path == path { old_path = file.old_path; } }
    let id = ctx.get_state("selection_id") + 1;
    ctx.set_state("selection_id", id); ctx.set_state("pending_path", path); ctx.set_state("pending_staged", staged); ctx.set_state("selecting", true);sync_update_guard(ctx);
    ctx.start_task("alloy.git", "execute", #{op:"select", root:ctx.get_state("snapshot").root, path:path, old_path:old_path, staged:staged, selection_id:id}, Fn("selected"), Fn("selection_failed"));
}
fn select(ctx, path) { select_file(ctx, path, ctx.get_state("tab") == "staged"); }
fn set_tab(ctx, value) { let d = ctx.get_state("doc"); if d != () { select_file(ctx, d.path, value == "staged"); } }
fn opened(ctx) { let tabs = ctx.get_state("open_tabs"); if tabs == () {[]} else {tabs} }
fn close_tab(ctx, path) {
    watch_invalidate(ctx);
    let d = ctx.get_state("doc");
    if ctx.get_state("selecting") || navigation_blocked(ctx) { return; }
    if d != () && d.path == path && dirty(ctx) { return; }
    let tabs = []; for item in opened(ctx) { if item != path { tabs.push(item); } }
    ctx.set_state("open_tabs", tabs);
    if d != () && d.path == path {
        if tabs.len() > 0 { select(ctx, tabs[tabs.len()-1]); }
        else { ctx.set_state("doc", ()); ctx.set_state("draft", ""); }
    }
}
fn mode_open(ctx, value) { ctx.set_state("mode_open",value); }
fn whitespace_open(ctx, value) { ctx.set_state("whitespace_open",value); }
fn whitespace_change(ctx, value) { if value != () {ctx.set_state("whitespace",value);} ctx.set_state("whitespace_open",false); }
fn file_name(path) { let parts = path.split("/"); parts[parts.len()-1] }
fn document_tabs(ctx,d) {
    let tabs = [];
    for path in opened(ctx) {
        let active = path == d.path;
        tabs.push(row([
            text(`⇄ Commit: ${file_name(path)}`).with_style(style().padding_x(px(10)).padding_y(px(8))).on_click_value(Fn("select"),path),
            text("×").with_style(style().padding_x(px(6)).padding_y(px(8)).text_color(theme_color("text_muted"))).accessibility_role("button").accessibility_label(`Close ${path}`).on_click_value(Fn("close_tab"),path)
        ]).with_key(`tab-${path}`).with_style(style().flex_shrink(false).items_center().border_bottom(px(3)).border_color(if active {theme_color("accent")} else {theme_color("surface")}).background(theme_color("surface"))));
    }
    row(tabs).with_key("document-tabs").with_style(style().height(px(38)).flex_shrink(false).overflow_x_scroll().background(theme_color("surface")))
}
fn toggle_unchanged(ctx, payload) { ctx.set_state("hide_unchanged", !ctx.get_state("hide_unchanged")); }
fn set_mode(ctx, value) { if value != () {ctx.set_state("mode", value);} ctx.set_state("mode_open",false); }
fn edit(ctx, payload) { ctx.set_state("editor", !ctx.get_state("editor")); }
fn base_toggle(ctx, payload) { ctx.set_state("base_view", !ctx.get_state("base_view")); }
fn save_data(ctx) { let d = ctx.get_state("doc"); #{expected:d.right, exists:d.exists, content:ctx.get_state("draft")} }
fn save(ctx, payload) { let d = ctx.get_state("doc"); if d != () && d.editable { request(ctx, "save", save_data(ctx)); } }
fn discard(ctx, payload) { ctx.set_state("confirm", "discard"); }
fn discard_now(ctx, payload) { ctx.set_state("draft", ctx.get_state("doc").right); ctx.set_state("confirm", ""); status(ctx, "Draft discarded; disk was not modified."); }
fn stage(ctx, payload) { if guard(ctx) { request(ctx, "stage", #{}); } }
fn unstage(ctx, payload) { if guard(ctx) { request(ctx, "unstage", #{}); } }
fn resolve(ctx, payload) { request(ctx, "resolve", save_data(ctx)); }
fn delete_prompt(ctx, payload) { ctx.set_state("confirm", "delete"); }
fn delete_now(ctx, payload) { request(ctx, "delete", save_data(ctx)); }
fn shortcut_label(ctx,label,action) {for item in ctx.get_state("shortcuts") {if item.action==action {return `${label}  ${item.display_key}`;}} label}
fn fetch(ctx,payload) { if ctx.get_state("snapshot") != () {network(ctx,"fetch");} }
fn network(ctx, op) { if guard(ctx) { request(ctx, op, #{}); } }
fn tag_prompt(ctx, payload) { if guard(ctx) && ctx.get_state("snapshot") != () { request(ctx,"tag_prepare",#{}); } }
fn tag_choose(ctx, level) { let t=ctx.get_state("tag"); if t == () {return;} ctx.set_state("tag_choice",t.versions[level]); ctx.set_state("confirm","tag-go"); }
fn tag_mode(ctx, mode) { let t=ctx.get_state("tag"); if t == () {return;} request(ctx,"tag_create",#{version:ctx.get_state("tag_choice"),head:t.head,push:mode=="push",remote:t.remote,push_url:t.push_url}); }
fn tag_retry(ctx, payload) { let t=ctx.get_state("tag"); if t == () {return;} request(ctx,"tag_push",#{version:t.version,commit:t.commit,remote:t.remote,push_url:t.push_url}); }
fn partials(ctx) { let value = ctx.get_state("partial"); if value == () {#{} } else {value} }
fn selected_count(ctx) { checked_paths(ctx).len() + partials(ctx).len() }
fn hunk_action(ctx, event) {
    let d = ctx.get_state("doc");
    if d == () || ctx.get_state("busy") || ctx.get_state("selecting") || dirty(ctx) { return; }
    if d.path != event.path || d.left != event.left || d.right != event.right { return; }
    if event.index < 0 || event.index >= d.hunks.len() { return; }
    if event.action == "restore" { restore(ctx,event.index); return; }
    if event.action != "toggle" { return; }
    if d.staged_hunks == () || d.index_source == () {status(ctx,"Refresh or use the file checkbox for this staged change.");return;}
    let indices = d.staged_hunks;
    if indices.contains(event.index) {let next=[];for i in indices {if i != event.index {next.push(i);}} indices=next;}
    else {indices.push(event.index);}
    request(ctx,"stage_hunks",#{selection:#{left:d.left,right:d.right,indices:indices},expected_index:d.index_source});
}
fn hunk_controls(ctx) {
    let d=ctx.get_state("doc");let result=[];
    if d == () || d.conflict || d.staged || d.added || !d.exists || dirty(ctx) || d.staged_hunks == () || ctx.get_state("whitespace") != "exact" || ctx.get_state("busy") || ctx.get_state("selecting") {return result;}
    let i=0;for h in d.hunks {result.push(#{index:i,old_start:h.old_start,new_start:h.start,checked:d.staged_hunks.contains(i)});i+=1;} result
}

fn inclusion(ctx) { let value = ctx.get_state("included"); if value == () {#{} } else {value} }
fn checked_paths(ctx) { let result = []; let checked = inclusion(ctx); for path in checked.keys() { if checked[path] { result.push(path); } } result }
fn toggle_include(ctx,path) {
    if !guard(ctx) {return;}
    let checked=inclusion(ctx);
    request(ctx,"stage_paths",#{paths:[path],checked:!(path in checked && checked[path])});
}
fn toggle_group(ctx,paths) {
    if !guard(ctx) || paths.len() == 0 {return;}
    let checked=inclusion(ctx);let all=true;
    for path in paths {if !(path in checked && checked[path]) {all=false;}}
    request(ctx,"stage_paths",#{paths:paths,checked:!all});
}
fn collapse_group(ctx, group) {
    let groups = ctx.get_state("collapsed"); if groups == () { groups = #{}; }
    groups[group] = !(group in groups && groups[group]); ctx.set_state("collapsed", groups);sync_file_rows(ctx);
}
fn staging_toggle(ctx, payload) { if guard(ctx) { ctx.set_state("advanced", !ctx.get_state("advanced")); if !ctx.get_state("advanced") { set_tab(ctx,"work"); } } }
fn commit(ctx, payload) { if guard(ctx) { request(ctx, "commit", #{message:ctx.get_state("commit")}); } }
fn commit_index(ctx, payload) { if guard(ctx) { request(ctx, "commit", #{message:ctx.get_state("commit")}); } }
fn navigate(ctx, direction) {
    let s = ctx.get_state("snapshot"); if s == () { return; }
    let files = []; let filter = ctx.get_state("filter").to_lower();
    for file in s.files { if filter == "" || file.path.to_lower().contains(filter) { files.push(file.path); } }
    let path = if ctx.get_state("selecting") {ctx.get_state("pending_path")} else {let d = ctx.get_state("doc"); if d == () {""} else {d.path}};
    let index = files.index_of(path); let next = index + direction;
    if next >= 0 && next < files.len() { select(ctx,files[next]); }
}
fn previous_file(ctx, payload) { navigate(ctx,-1); }
fn next_file(ctx, payload) { navigate(ctx,1); }
fn restore(ctx, number) { request(ctx, "hunk", #{left:ctx.get_state("doc").left,content:ctx.get_state("draft"),number:number}); }
fn choose(ctx, data) { request(ctx, "choose", #{content:ctx.get_state("draft"),number:data.number,choice:data.choice}); }
fn accept_file(ctx, side) { let d = ctx.get_state("doc"); if merge_side_deleted(ctx,d,side) {ctx.set_state("confirm","delete");return;} ctx.set_state("draft", if side == "ours" {d.ours} else {d.theirs}); ctx.set_state("editor",true); status(ctx,"Replaced the draft with the selected side. Review before marking resolved."); }
fn small(value) { text(value).with_style(style().typography("caption").text_color(theme_color("text_muted"))) }
fn button(ctx, label, handler, payload, enabled) {
    text(label).with_style(style().font_size(px(12)).line_height(px(18)).padding_x(px(7)).padding_y(px(3)).radius(px(3))
        .background(theme_color("surface_raised"))
        .hover(style().background(theme_color("surface_hover"))).disabled(style().opacity(0.4))).disabled(!enabled || ctx.get_state("busy"))
        .accessibility_role("button").accessibility_label(label).on_click_value(handler,payload)
}
fn navigation_button(ctx,label,handler,payload,enabled) {
    button(ctx,label,handler,payload,enabled).disabled(!enabled || navigation_blocked(ctx))
}
fn tab(ctx, label, value) {
    navigation_button(ctx,label,Fn("set_tab"),value,true).with_style(style().background(if ctx.get_state("tab") == value {theme_color("selection")} else {theme_color("surface_raised")}))
}
fn image_zoom(ctx, payload) { ctx.set_state("image_actual", !ctx.get_state("image_actual")); }
fn image_pane(ctx, key, preview, label, single) {
    let nodes = [text(label).with_style(style().padding(px(8)).background(theme_color("surface_raised")))];
    if preview == () { nodes.push(small("No image in this version")); }
    else if preview.error != "" { nodes.push(text(preview.error).with_style(style().padding(px(16)).text_color(theme_color("warning")))); }
    else {
        nodes.push(small(`${preview.format} · ${preview.width} × ${preview.height} · ${preview.bytes / 1024} KB`).with_style(style().padding_x(px(8)).padding_y(px(4))));
        let w = preview.width.to_float(); let h = preview.height.to_float();
        if !ctx.get_state("image_actual") {
            let limit = if single {760.0} else {400.0};
            let scale = min(1.0, min(limit / w, 540.0 / h)); w *= scale; h *= scale;
        }
        nodes.push(column([image_source(asset(preview.asset)).with_style(style().width(px(w)).height(px(h)).flex_shrink(false))])
            .with_key(key).with_style(style().flex_grow().min_height(px(0)).min_width(px(0)).padding(px(12)).overflow_x_scroll().overflow_y_scroll().background(rgba(0x303236ff))));
    }
    column(nodes).with_style(style().flex_basis(px(0)).flex_grow().min_width(px(0)).min_height(px(0)))
}
fn viewer(ctx,key,left,right,left_label,right_label,file,mode) {
    diff::DiffViewer(#{key:`${key}-${ctx.get_state("hide_unchanged")}`,left:#{source:left,label:left_label,file_name:file},right:#{source:right,label:right_label,file_name:file},mode:mode,context_lines:if ctx.get_state("hide_unchanged") {3} else {"all"},whitespace:ctx.get_state("whitespace"),hunk_controls:hunk_controls(ctx),show_hunk_controls:ctx.get_state("doc") != () && !ctx.get_state("doc").conflict && !ctx.get_state("doc").staged,on_hunk_action:Fn("hunk_action")})
        .tab_index(0).with_style(style().flex_grow().min_height(px(0)).min_width(px(0)))
}
fn editor(ctx,d) {
    column([
        row([text(if d.conflict {"MERGE RESULT"} else {"WORKING TREE · EDIT"}).with_style(style().font_weight(600)),small(if dirty(ctx) {"● Unsaved draft"} else {"Saved"})]).with_style(style().justify_between()),
        textarea::Textarea(#{ key:`editor-${d.path}-${ctx.get_state("focus_editor_id")}`, autofocus:ctx.get_state("focus_editor_id")>0, label:"Editable file content", value:ctx.get_state("draft"), rows:15, on_change:Fn("draft_change"), disabled:ctx.get_state("busy") || ctx.get_state("selecting"), part_styles:#{editor:style().font_family("Menlo").font_size(px(12))} })
    ]).with_style(style().gap(px(4)).padding(px(6)).background(theme_color("surface_raised")))
}
fn tag_bar_style() { style().width(relative(1)).padding_x(px(8)).padding_y(px(4)).gap(px(6)).items_center().overflow_x_scroll().background(theme_color("selection")) }
fn tag_bump_bar(ctx) {
    let t = ctx.get_state("tag"); if t == () { return row([]); }
    row([text(`Tag ${t.branch} @ ${t.head_short}`).with_style(style().max_width(px(190)).min_width(px(0)).text_ellipsis().overflow_hidden()),
        text(t.summary).with_style(style().max_width(px(280)).min_width(px(0)).text_ellipsis().overflow_hidden().text_color(theme_color("text_muted"))),
        small(`base ${t.base}`),
        button(ctx,`s → ${t.versions.s}`,Fn("tag_choose"),"s",true),
        button(ctx,`m → ${t.versions.m}`,Fn("tag_choose"),"m",true),
        button(ctx,`l → ${t.versions.l}`,Fn("tag_choose"),"l",true),
        button(ctx,"Cancel",Fn("cancel"),(),true)]).with_style(tag_bar_style())
}
fn tag_go_bar(ctx) {
    let t = ctx.get_state("tag"); if t == () { return row([]); }
    let nodes = [text(`Create tag ${ctx.get_state("tag_choice")} at ${t.head_short}?`)];
    if t.remote != "" { nodes.push(button(ctx,`Create and push to ${t.remote}`,Fn("tag_mode"),"push",true)); }
    nodes.push(button(ctx,"Create local tag only",Fn("tag_mode"),"local",true));
    nodes.push(button(ctx,"Cancel",Fn("cancel"),(),true));
    row(nodes).with_style(tag_bar_style())
}
fn tag_retry_bar(ctx) {
    let t = ctx.get_state("tag"); if t == () { return row([]); }
    row([text(`Tag ${t.version} exists locally; push failed. Retry or cancel (the tag is kept).`),
        button(ctx,`Retry push to ${t.remote}`,Fn("tag_retry"),(),true),
        button(ctx,"Cancel",Fn("cancel"),(),true)]).with_style(tag_bar_style())
}
fn confirmation(ctx) {
    let c = ctx.get_state("confirm");
    if c == "" { return row([]); }
    if c == "sensitive-commit" {
        let review=ctx.get_state("sensitive_review"); if review==() {return row([]);}
        let files=""; for path in review.files {if files!="" {files+="\n";} files+=path;}
        return column([
            text("AI 提交已拦截：以下文件名命中敏感文件规则").with_style(style().font_weight(700).text_color(theme_color("danger"))),
            text(files).with_style(style().max_height(px(120)).overflow_y_scroll()),
            text("请人工确认这些文件可以发送给所选 ACP 代理或直连模型并提交到 Git。授权仅对本次暂存内容有效。"),
            row([button(ctx,"确认并继续 AI 提交",Fn("sensitive_commit_now"),(),true),button(ctx,"取消",Fn("cancel"),(),true)]).with_style(style().gap(px(8)))
        ]).with_style(style().padding(px(12)).gap(px(8)).flex_shrink(false).background(theme_color("surface_raised")).border(px(1)).border_color(theme_color("danger")));
    }
    if c == "tag" { return tag_bump_bar(ctx); }
    if c == "tag-go" { return tag_go_bar(ctx); }
    if c == "tag-retry" { return tag_retry_bar(ctx); }
    let label = if c == "abort-merge" {"Abort this Git merge and restore the pre-merge state?"} else if c == "rollback-all" {"Roll back all committed files to HEAD? New files will be kept."} else if c == "delete-file" {`Delete ${ctx.get_state("delete_path")} from disk?`} else if c == "rollback" {`Roll back ${ctx.get_state("rollback_path")} to HEAD?`} else if c == "push" {"No upstream is configured. Push this branch and set its upstream on the configured push remote?"} else if c == "pull" {"Pull and merge changes from the configured upstream?"} else if c == "close" {"Close Alloy and discard unsaved work? An in-flight Git operation may still finish."} else if c == "delete" {"Delete this conflicted file and stage the deletion?"} else {"Discard your unsaved draft?"};
    let handler = if c == "abort-merge" {Fn("abort_merge_now")} else if c == "delete-file" {Fn("delete_file_now")} else if c == "rollback" || c == "rollback-all" {Fn("rollback_now")} else if c == "push" || c == "pull" {Fn("network_confirm")} else if c == "close" {Fn("close_now")} else if c == "delete" {Fn("delete_now")} else {Fn("discard_now")};
    row([text(label),button(ctx,"Confirm",handler,(),true),button(ctx,"Cancel",Fn("cancel"),(),true)])
        .with_style(style().padding(px(12)).gap(px(10)).items_center().background(theme_color("selection")))
}
fn has_message(ctx) { let value = ctx.get_state("commit"); value.trim(); value != "" }
fn check_box(ctx, label, checked, mixed, handler, payload, enabled) {
    text(if mixed {"−"} else if checked {"✓"} else {""}).with_style(style().width(px(16)).height(px(16)).flex_shrink(false).text_center().font_size(px(12)).radius(px(3)).border(px(1)).border_color(theme_color("text_muted")))
        .accessibility_role("checkbox").accessibility_label(label).accessibility_checked(if mixed {"mixed"} else {checked})
        .disabled(!enabled || ctx.get_state("busy")).on_click_value(handler,payload)
}
fn file_group(file) { if file.conflict {"Merge Conflicts"} else if file.index == "?" {"Unversioned Files"} else {"Changes"} }
fn awake_agents(ctx) {let agents=ctx.get_state("awake_agents");if agents==() {[]} else {agents}}
fn awake_update(ctx,result) {
    ctx.set_state("awake_enabled",result.enabled);ctx.set_state("awake_active",result.active);
    ctx.set_state("awake_agents",result.agents);ctx.set_state("awake_error",result.error);
    let detail=if !result.enabled {"防休眠已关闭；勾选后在 Agent 工作时保持系统唤醒。"} else if result.active {"正在保持系统和屏幕唤醒，防止闲置熄屏后自动锁屏；仍可手动锁屏。"} else {"防休眠已启用，等待 Agent 开始工作。"};
    if result.error!="" {detail+=" 检测异常："+result.error;}
    ctx.set_state("awake_detail",detail);
}
fn awake_toggle(ctx,payload) {
    try {
        awake_update(ctx,ctx.call_capability("alloy.awake","set_enabled",!ctx.get_state("awake_enabled")));
        if ctx.get_state("awake_enabled") {ctx.set_state("awake_open",true);}
    }
    catch(error) {status(ctx,`保持唤醒：${error}`);}
}
fn awake_open_changed(ctx,open) {ctx.set_state("awake_open",open);}
fn awake_menu(ctx) {
    if !ctx.get_state("awake_open") {return row([]);}
    let agents=awake_agents(ctx);
    let nodes=[
        row([text("工作中的 Agent").with_style(style().font_size(px(12)).font_weight(600)),text(`${agents.len()}`).with_style(style().font_size(px(11)).text_color(theme_color("text_muted")))]).with_style(style().gap(px(6)).items_center()),
        text(ctx.get_state("awake_detail")).with_style(style().font_size(px(11)).line_height(px(16)).text_color(theme_color("text_muted")))
    ];
    if agents.len()==0 {nodes.push(text("暂无正在工作的 Agent").with_style(style().font_size(px(12)).padding_y(px(12)).text_color(theme_color("text_muted"))));}
    for agent in agents {
        nodes.push(column([
            row([text("Agent").with_style(style().font_size(px(10)).width(px(36)).flex_shrink(false).text_color(theme_color("text_muted"))),text(agent.name).with_style(style().font_size(px(12)).font_weight(600).line_height(px(18)))]).with_style(style().min_height(px(18)).gap(px(6)).items_center()),
            row([text("标题").with_style(style().font_size(px(10)).width(px(36)).flex_shrink(false).text_color(theme_color("text_muted"))),text(agent.title).with_style(style().font_size(px(12)).line_height(px(18)))]).with_style(style().min_height(px(18)).gap(px(6))),
            row([text("路径").with_style(style().font_size(px(10)).width(px(36)).flex_shrink(false).text_color(theme_color("text_muted"))),text(agent.project).with_style(style().font_size(px(10)).line_height(px(18)).min_width(px(0)).text_color(theme_color("text_muted")).text_ellipsis().overflow_hidden())]).with_style(style().min_height(px(18)).gap(px(6)).items_center())
        ]).with_key(`working-agent-${agent.id}`).with_style(style().padding_y(px(8)).gap(px(3)).min_width(px(0)).border_top(px(1)).border_color(theme_color("border"))));
    }
    column(nodes).with_key("awake-agent-list").accessibility_role("dialog").accessibility_label("工作中的 Agent 列表")
      .with_style(style().width(px(420)).max_height(px(360)).overflow_y_scroll().padding_x(px(12)).padding_y(px(10)).gap(px(8)).radius(px(5)).border(px(1)).border_color(theme_color("border")).background(theme_color("surface_raised")).text_color(theme_color("text_primary")))
}
fn awake_checkbox(ctx) {
    let checked=ctx.get_state("awake_enabled");let agents=awake_agents(ctx);
    let checkbox=text(if checked {"✓"} else {""}).with_key("keep-awake-checkbox")
      .with_style(style().width(px(14)).height(px(14)).line_height(px(12)).flex_shrink(false).text_center().font_size(px(11)).font_weight(400).radius(px(3)).border(px(1)).border_color(if checked {theme_color("accent")} else {theme_color("text_muted")}).background(if checked {theme_color("accent")} else {theme_color("surface")}).text_color(theme_color("on_accent")))
      .accessibility_role("checkbox").accessibility_label("Codex / Herdr 工作时保持唤醒").accessibility_checked(checked).tab_index(0)
      .on_key_value("space",Fn("awake_toggle"),()).on_key_value("enter",Fn("awake_toggle"),()).on_click(Fn("awake_toggle"));
    let trigger=row([
        text("保持唤醒").with_style(style().font_size(px(12)).font_weight(400).line_height(px(18))),
        text(`${agents.len()}`).with_key("working-agent-count").with_style(style().font_size(px(10)).font_weight(500).line_height(px(14)).min_width(px(18)).padding_x(px(4)).text_center().radius(px(8)).background(theme_color("surface")).text_color(if ctx.get_state("awake_active") {theme_color("success")} else {theme_color("text_muted")})),
        text("▾").with_style(style().font_size(px(10)).font_weight(400).text_color(theme_color("text_muted")))
    ]).with_style(style().height(px(24)).gap(px(5)).items_center().hover(style().background(theme_color("surface_hover"))))
      .accessibility_role("button").accessibility_label(`查看 ${agents.len()} 个工作中的 Agent`);
    row([checkbox,overlay(trigger,awake_menu(ctx),#{id:"awake-agents",kind:"popover",placement:"bottom",open:ctx.get_state("awake_open"),gap:6,modal:false,dismiss_on_escape:true,dismiss_on_outside:true,match_trigger_width:false,initial_focus:"first"}).on_open_change(Fn("awake_open_changed"))])
      .with_key("keep-awake").with_style(style().height(px(24)).flex_shrink(false).padding_x(px(7)).gap(px(6)).items_center().radius(px(3)).background(theme_color("surface_raised")))
}

fn sync_file_rows(ctx) {
    let s=ctx.get_state("snapshot");
    if s==() || s.files.len()<=200 {ctx.set_state("large_file_rows",());return;}
    ctx.set_state("large_file_rows",ctx.call_capability("alloy.boot","file_rows",#{files:s.files,checked:inclusion(ctx),partial:partials(ctx),collapsed:ctx.get_state("collapsed"),filter:ctx.get_state("filter")}));
}
fn sidebar_file(ctx,f,merging,included,mixed) {
    let selected = if ctx.get_state("selecting") {ctx.get_state("pending_path") == f.path} else {ctx.get_state("doc") != () && ctx.get_state("doc").path == f.path};
    let color = if f.conflict || f.index == "D" || f.work == "D" {theme_color("danger")} else if f.index == "?" || f.index == "A" {theme_color("success")} else {theme_color("accent")};
    let parts = f.path.split("/"); let name = parts[parts.len()-1];
    let directory = if parts.len() > 1 { f.path.sub_string(0, f.path.len()-name.len()-1) } else {""};
    let status = if f.conflict {"!"} else if f.index == "?" {"?"} else if f.index != " " {f.index} else {f.work};
    let label = row([
        text(status).with_style(style().width(px(14)).flex_shrink(false).font_family("Menlo").font_size(px(11)).text_center().text_color(color)),
        text(name).with_style(style().flex_shrink(false).text_color(color)),
        text(directory).with_style(style().text_color(theme_color("text_muted")).min_width(px(0)).text_ellipsis().overflow_hidden())
    ]).with_style(style().height(relative(1)).flex_grow().min_width(px(0)).gap(px(5)).items_center().whitespace_nowrap().overflow_hidden())
        .accessibility_label(f.path).on_click_value(Fn("select"),f.path).disabled(navigation_blocked(ctx));
    let row_style = style().height(px(26)).min_height(px(26)).flex_shrink(false).padding_left(px(22)).padding_right(px(6)).gap(px(6)).items_center();
    let row_style = row_style.radius(px(3)).background(if selected {theme_color("selection")} else {theme_color("surface_raised")}).hover(style().background(theme_color("surface_hover")));
    let file_row = row([check_box(ctx,`Stage ${f.path}`,included,mixed,Fn("toggle_include"),f.path,!f.conflict && !merging),label]).with_key(`file-${f.path}`).with_style(row_style);
    context_menu::ContextMenu(#{key:`file-menu-${f.path}`,label:`Actions for ${f.path}`,trigger:file_row,open:ctx.get_state("file_menu")==f.path,active_value:"open-file",items:[#{kind:"item",value:"open-file",label:"Open",disabled:f.work == "D" || f.index == "D" && f.work == " "},#{kind:"item",value:"open-folder",label:"Open Folder"},#{kind:"separator"},#{kind:"item",value:"add-to-gitignore",label:"Add to .gitignore",disabled:f.path == ".gitignore" || merging || ctx.get_state("busy") || ctx.get_state("selecting") || dirty(ctx)},#{kind:"item",value:"rollback",label:"Rollback",disabled:!can_rollback(ctx,f.path) || merging || ctx.get_state("busy") || ctx.get_state("selecting") || dirty(ctx)},#{kind:"item",value:"delete-file",label:"Delete",disabled:f.work == "D" || f.index == "D" && f.work == " " || merging || ctx.get_state("busy") || ctx.get_state("selecting") || dirty(ctx)}],on_open_change:Fn("file_menu_open").curry(f.path),on_action:Fn("file_action").curry(f.path)})
}
fn sidebar_virtual_row(merging,ctx,payload) {
    let item=payload.item;
    if item.kind=="file" {return sidebar_file(ctx,item.file,merging,item.included,item.mixed);}
    sidebar_group(ctx,item.group,item.count,item.total,item.mixed,item.folded,item.paths,merging)
}
fn sidebar_virtual_list(ctx,s,items) {
    let config=#{key:`file-list-${s.root}`,label:"Changed files",data:items,estimated_height:26,fill_height:true,overdraw_pixels:104};
    let d=ctx.get_state("doc");
    if ctx.get_state("selecting") {config.reveal_key=`file-${ctx.get_state("pending_path")}`;}
    else if d!=() {config.reveal_key=`file-${d.path}`;}
    virtual_collection(config,Fn("sidebar_virtual_row").curry(s.merging)).with_key("file-list").with_style(style().flex_grow().min_height(px(0)))
}
fn toggle_visible_group(ctx,group) {
    let paths=ctx.call_capability("alloy.boot","file_rows",#{files:ctx.get_state("snapshot").files,group:group,filter:ctx.get_state("filter")});
    toggle_group(ctx,paths);
}
fn sidebar_group(ctx,group,count,total,mixed,folded,paths,merging) {
    let title=row([text(if folded {"›"} else {"⌄"}),text(group).with_style(style().font_weight(600)),small(`${total} files`)])
        .with_style(style().flex_grow().gap(px(6)).items_center()).on_click_value(Fn("collapse_group"),group);
    row([check_box(ctx,`Stage ${group}`,count==total && !mixed,mixed || (count>0 && count<total),if paths==() {Fn("toggle_visible_group")} else {Fn("toggle_group")},if paths==() {group} else {paths},group!="Merge Conflicts" && !merging),title])
        .with_key(`group-${group}`).with_style(style().height(px(28)).min_height(px(28)).flex_shrink(false).padding_x(px(6)).gap(px(6)).items_center())
}

fn sidebar(ctx,s) {
    let virtualized=s.files.len()>200;
    let items = []; let checked = inclusion(ctx); let partial = partials(ctx); let groups = ctx.get_state("collapsed");
    let filter = ctx.get_state("filter").to_lower();
    if virtualized {
        items=ctx.get_state("large_file_rows");
    } else {
        for group in ["Merge Conflicts", "Changes", "Unversioned Files"] {
            let files = []; let paths = []; let count = 0; let mixed = false;
            for file in s.files {
                if file_group(file) != group || (filter != "" && !file.path.to_lower().contains(filter)) { continue; }
                files.push(file); paths.push(file.path);
                if file.path in checked && checked[file.path] { count += 1; }
                if file.path in partial { mixed = true; }
            }
            if files.len() == 0 { continue; }
            let folded = groups != () && group in groups && groups[group];
            items.push(sidebar_group(ctx,group,count,files.len(),mixed,folded,paths,s.merging));
            if folded { continue; }
            for f in files {
                let included=f.path in checked && checked[f.path];let mixed=f.path in partial;
                items.push(sidebar_file(ctx,f,s.merging,included,mixed));
            }
        }
    }
    let can_commit = has_message(ctx) && selected_count(ctx) > 0;
    let footer = [textarea::Textarea(#{key:`commit-${ctx.get_state("focus_commit_id")}`,autofocus:ctx.get_state("focus_commit_id")>0,label:"Commit message",value:ctx.get_state("commit"),placeholder:"生成的提交信息会显示在这里",read_only:!s.merging,on_change:Fn("commit_change"),min_rows:4,max_rows:10})];
    if s.merging {
        let unresolved=conflict_count(s);
        footer.push(small(if unresolved>0 {`${unresolved} conflict files remaining`} else {"All conflicts resolved · commit to finish the merge"}));
        footer.push(button(ctx,"Commit merge",Fn("commit_index"),(),unresolved==0 && has_message(ctx)));
        footer.push(button(ctx,"Abort merge…",Fn("abort_merge_prompt"),(),true));
    }
    else { footer.push(button(ctx,`Commit ${selected_count(ctx)} staged files`,Fn("commit"),(),can_commit)); }
    if ctx.get_state("advanced") && !s.merging { footer.push(button(ctx,"Commit staged index",Fn("commit_index"),(),has_message(ctx))); }
    footer.push(row([button(ctx,shortcut_label(ctx,"Commit","alloy.ai_commit"),Fn("ai_commit"),(),true),acp_agent_selector(ctx)]).with_style(style().gap(px(8)).items_center()));
    column([
        row([text(if s.merging {"Merge"} else {"Commit"}).with_style(style().font_size(px(14)).font_weight(600)),button(ctx,"Refresh",Fn("refresh"),(),true),button(ctx,"Rollback all",Fn("rollback_all"),(),!s.merging && s.files.len()>0),button(ctx,if ctx.get_state("advanced") {"Hide staging"} else {"Staging…"},Fn("staging_toggle"),(),true)]).with_style(style().padding_x(px(6)).gap(px(6)).items_center()),
        input::Input(#{key:`filter-${ctx.get_state("focus_filter_id")}`,autofocus:ctx.get_state("focus_filter_id")>0,label:"Filter changed files",value:ctx.get_state("filter"),placeholder:"Filter files…",on_change:Fn("filter_change"),part_styles:#{input:style().height(px(26)).font_size(px(12))}}),
        if virtualized {sidebar_virtual_list(ctx,s,items)}
        else {column(items).with_key("file-list").with_style(style().flex_grow().min_height(px(0)).overflow_y_scroll())},
        column(footer).with_style(style().border_top(px(1)).border_color(theme_color("border")).padding_top(px(6)).gap(px(5)))
    ]).with_style(style().width(px(350)).flex_shrink(false).padding(px(6)).gap(px(4)).border_right(px(1)).border_color(theme_color("border")).background(theme_color("surface_raised")))
}
fn diff_path_bar(ctx,path,old_path,key) {
    let root=ctx.get_state("snapshot").root;
    let full_path=`${root}/${path}`;
    let label=if old_path=="" {full_path} else {`${root}/${old_path} → ${full_path}`};
    let trigger=text(label).accessibility_label(`File path: ${label}`).with_style(style().font_size(px(11)).font_family("Menlo").min_width(px(0)).whitespace_nowrap().text_ellipsis().overflow_hidden().text_color(theme_color("text_muted")));
    let path_node=tooltip::Tooltip(#{key:key,label:label,trigger:trigger,content:text(label),placement:"bottom"}).with_style(style().flex_grow().min_width(px(0)));
    row([path_node]).with_key(`${key}-bar`).with_style(style().height(px(26)).min_height(px(26)).flex_shrink(false).min_width(px(0)).padding_x(px(10)).items_center().background(theme_color("surface_raised")))
}
fn conflict_count(s) {let count=0;for file in s.files {if file.conflict {count+=1;}} count}
fn merge_markers(ctx) {for line in ctx.get_state("draft").split("\n") {for prefix in ["<<<<<<<","|||||||","=======",">>>>>>>"] {if line.starts_with(prefix) {return true;}}} false}
fn abort_merge_prompt(ctx,payload) {if guard(ctx) {ctx.set_state("confirm","abort-merge");}}
fn abort_merge_now(ctx,payload) {if guard(ctx) {request(ctx,"abort_merge",#{});}}
fn merge_side_deleted(ctx,d,side) {for file in ctx.get_state("snapshot").files {if file.path==d.path && file.conflict {return if side=="ours" {file.index=="D"} else {file.work=="D"};}} false}
fn merge_source(ctx,d,key,label,source,side) {
    let header=row([text(label).with_style(style().font_weight(600).flex_grow()),button(ctx,if merge_side_deleted(ctx,d,side) {"Keep deletion"} else if side=="ours" {"Use Local"} else {"Use Incoming"},Fn("accept_file"),side,true)]).with_style(style().height(px(32)).padding_x(px(6)).gap(px(6)).items_center().background(theme_color("surface_raised")));
    let source_view=code::CodeViewer(#{key:key,source:source,file_name:d.path,label:label,show_line_numbers:true,wrap:"none"}).with_style(style().flex_grow().min_width(px(0)).min_height(px(0)));
    column([header,source_view]).with_key(`${key}-pane`).with_style(style().flex_basis(px(0)).flex_grow().min_width(px(0)).min_height(px(0)))
}
fn merge_view(ctx,d) {
    let result_header=row([text("RESULT").with_style(style().font_weight(600).flex_grow()),small(if dirty(ctx) {"● Unsaved"} else {"Saved"})]).with_style(style().height(px(32)).padding_x(px(6)).gap(px(6)).items_center().background(theme_color("selection")));
    let result_editor=textarea::Textarea(#{key:`merge-result-${d.path}`,label:"Final merged version",value:ctx.get_state("draft"),rows:24,on_change:Fn("draft_change"),disabled:ctx.get_state("busy") || ctx.get_state("selecting"),part_styles:#{root:style().flex_grow().min_height(px(0)),editor:style().font_family("Menlo").font_size(px(12)).radius(px(0)).border(px(0)).height(relative(1))}}).with_style(style().flex_grow().min_height(px(0)));
    let result=column([result_header,result_editor]).with_key("merge-result-pane").with_style(style().flex_basis(px(0)).flex_grow().min_width(px(0)).min_height(px(0)).overflow_y_scroll());
    row([merge_source(ctx,d,"merge-local","LOCAL",d.ours,"ours"),result,merge_source(ctx,d,"merge-incoming","INCOMING",d.theirs,"theirs")]).with_key("three-way-merge").with_style(style().flex_grow().min_width(px(0)).min_height(px(0)).gap(px(1)))
}
fn content(ctx) {
    let d = ctx.get_state("doc");
    if d == () {
        return column([text("Select a file to review").with_style(style().typography("display")),small("Choose a changed file on the left. File changes refresh automatically.")])
            .with_style(style().flex_grow().items_center().justify_center().gap(px(12)));
    }
    let toolbar = [
        navigation_button(ctx,"←",Fn("previous_file"),(),true), navigation_button(ctx,"→",Fn("next_file"),(),true),
        button(ctx,if ctx.get_state("editor") {"Hide editor"} else {"Edit"},Fn("edit"),(),d.editable),
        select::Select(#{key:"viewer-mode",label:"Diff layout",options:[#{value:"split",label:"Side-by-side viewer"},#{value:"unified",label:"Unified viewer"}],value:ctx.get_state("mode"),open:ctx.get_state("mode_open"),query:"",empty_text:"No options",size:"xs",width:px(176),on_change:Fn("set_mode"),on_open_change:Fn("mode_open")}),
        select::Select(#{key:"whitespace",label:"Whitespace comparison",options:[#{value:"exact",label:"Do not ignore"},#{value:"ignore_changes",label:"Ignore changes"},#{value:"ignore_all",label:"Ignore all spaces"}],value:ctx.get_state("whitespace"),open:ctx.get_state("whitespace_open"),query:"",empty_text:"No options",size:"xs",width:px(156),on_change:Fn("whitespace_change"),on_open_change:Fn("whitespace_open")})
    ];
    toolbar.push(navigation_button(ctx,if ctx.get_state("hide_unchanged") {"Show all lines"} else {"Hide unchanged"},Fn("toggle_unchanged"),(),true));
    if d.images != () { toolbar = [navigation_button(ctx,"←",Fn("previous_file"),(),true),navigation_button(ctx,"→",Fn("next_file"),(),true),small("Image preview"),navigation_button(ctx,if ctx.get_state("image_actual") {"Reduced size"} else {"Actual size (100%)"},Fn("image_zoom"),(),true)]; }
    if d.conflict {toolbar=[navigation_button(ctx,"←",Fn("previous_file"),(),true),navigation_button(ctx,"→",Fn("next_file"),(),true),small("Merge conflict · edit the center result")];}
    if dirty(ctx) { toolbar.push(button(ctx,"Save",Fn("save"),(),d.editable)); toolbar.push(button(ctx,"Discard",Fn("discard"),(),true)); }
    if ctx.get_state("advanced") && !d.conflict {
        toolbar.push(tab(ctx,"Local","work")); toolbar.push(tab(ctx,"Staged","staged"));
        toolbar.push(button(ctx,"Stage",Fn("stage"),(),!d.conflict)); toolbar.push(button(ctx,"Unstage",Fn("unstage"),(),!d.conflict));
    }
    let old_path="";for file in ctx.get_state("snapshot").files {if file.path==d.path {old_path=file.old_path;}}
    let children = [document_tabs(ctx,d),diff_path_bar(ctx,d.path,old_path,"work-diff-path"),row(toolbar).with_style(style().height(px(34)).flex_shrink(false).gap(px(4)).padding_x(px(6)).items_center().overflow_x_scroll().background(theme_color("surface_raised")))];
    if d.images != () {
        let panes = [];
        if !d.added { panes.push(image_pane(ctx,`image-left-${d.path}`,d.images.left,"HEAD",false)); }
        panes.push(image_pane(ctx,`image-right-${d.path}`,d.images.right,if d.staged {"Staged version"} else {"Current version"},d.added));
        children.push(row(panes).with_style(style().flex_grow().min_width(px(0)).min_height(px(0)).gap(px(1))));
    }
    else if d.note != "" { children.push(text(d.note).with_style(style().padding(px(24)).text_color(theme_color("warning")))); }
    else if d.conflict {
        children.push(row([small("MERGE · Local → Result ← Incoming"),button(ctx,"Base",Fn("base_toggle"),(),true),button(ctx,"Mark resolved",Fn("resolve"),(),!merge_markers(ctx)),button(ctx,"Resolve as deletion",Fn("delete_prompt"),(),true)]).with_style(style().gap(px(6)).items_center().overflow_x_scroll()));
        children.push(merge_view(ctx,d));
        if ctx.get_state("base_view") {
            children.push(code::CodeViewer(#{key:`base-${d.path}`,source:d.base,file_name:d.path,label:"COMMON ANCESTOR"}).with_style(style().height(px(160))));
        }
        let actions = [];
        let number = 0;
        for line in ctx.get_state("draft").split("\n") {
            if line.starts_with("<<<<<<<") {
                actions.push(small(`Block ${number+1}`));
                for choice in ["ours","theirs","both"] { let label=if choice=="ours" {"Local"} else if choice=="theirs" {"Incoming"} else {"Both"};actions.push(button(ctx,label,Fn("choose"),#{number:number,choice:choice},true)); }
                number += 1;
            }
        }
        children.push(row(actions).with_key("conflict-actions").with_style(style().gap(px(6)).overflow_x_scroll().min_height(px(28))));
    } else {
        if d.added {
            children.push(row([text(if d.staged {"Staged version"} else {"Current version"}),small(d.path)]).with_style(style().height(px(26)).flex_shrink(false).padding_x(px(8)).gap(px(8)).items_center().background(theme_color("surface_raised"))));
            children.push(code::CodeViewer(#{key:`added-${d.path}-${d.staged}`,source:ctx.get_state("draft"),file_name:d.path,label:"Current version",part_styles:#{root:style().border(px(0)),line:style().background(rgba(0x284434ff)),gutter:style().background(rgba(0x284434ff)),text:style().font_family("Menlo").font_size(px(13))}}).tab_index(0).with_style(style().flex_grow().min_height(px(0)).min_width(px(0))));
        } else {
            children.push(viewer(ctx,`diff-${d.path}-${d.staged}`,d.left,ctx.get_state("draft"),"HEAD",if d.staged {"Staged version"} else {"Current version"},d.path,ctx.get_state("mode")));
        }
        if ctx.get_state("whitespace") != "exact" { children.push(small("Choose Do not ignore to select or restore exact change blocks.")); }

    }
    if ctx.get_state("editor") && d.editable && !d.conflict { children.push(editor(ctx,d)); }
    column(children).disabled(ctx.get_state("selecting")).with_style(style().flex_grow().min_width(px(0)).min_height(px(0)))
}
fn project_form(ctx) {
    if !ctx.get_state("add_project") { return row([]); }
    row([
        input::Input(#{key:"repository",label:"Repository path",value:ctx.get_state("root"),placeholder:"/path/to/repository",on_change:Fn("root_change"),on_submit:Fn("open_repo"),disabled:ctx.get_state("busy")}).with_style(style().flex_grow()),
        button(ctx,"Open",Fn("open_repo"),(),true),button(ctx,"Browse…",Fn("browse"),(),true),button(ctx,"Cancel",Fn("project_add"),(),true)
    ]).with_style(style().padding_x(px(10)).padding_y(px(4)).gap(px(6)).items_center())
}
fn project_menu(ctx) {
    if !ctx.get_state("projects_open") { return row([]); }
    let nodes = [small(if ctx.get_state("summaries_error")!="" {ctx.get_state("summaries_error")} else if ctx.get_state("projects_loading") || ctx.get_state("summaries_pending") {"Refreshing project summaries…"} else {"Recent projects · refreshed automatically · remote counts reflect the last Fetch"}).with_style(style().padding_x(px(10)))];
    let s = ctx.get_state("snapshot");
    for p in project_list(ctx) {
        let active = s != () && s.root == p.root;
        let counts=if p.error!="" {text("! Repository unavailable · choose to retry").with_style(style().text_color(theme_color("danger")).font_weight(600))} else {git_metrics(`project-status-${p.root}`,p,false)};
        nodes.push(column([
            row([text(if active {`✓ ${p.name}`} else {p.name}).with_style(style().font_weight(600)),small(p.branch),small(p.root)]).with_style(style().gap(px(12)).items_center()),
            counts
        ]).with_key(`project-${p.root}`).with_style(style().padding_x(px(12)).padding_y(px(6)).gap(px(2)).background(if active {theme_color("selection")} else {theme_color("surface_raised")}).hover(style().background(theme_color("surface_hover"))))
        .tab_index(0).on_key_value("enter",Fn("project_choose"),p.root).accessibility_role("button").accessibility_label(`Open project ${p.root}`).on("pointer_up",Fn("project_pointer").curry(p.root)).disabled(ctx.get_state("busy") || ctx.get_state("selecting")));
    }
    if project_list(ctx).len() == 0 && !ctx.get_state("projects_loading") { nodes.push(small("No recent projects. Add a repository to begin.")); }
    column(nodes).with_key("project-menu").with_style(style().width(px(960)).max_height(px(360)).overflow_y_scroll().flex_shrink(false).padding(px(6)).gap(px(3)).background(theme_color("surface_raised")))
}
fn acp_config_page(ctx) {
    let agents=acp_agent_list(ctx);let rows=[];
    for agent in agents {
        rows.push(row([
            column([text(agent.name).with_style(style().font_weight(600)),small(acp_agent_command(agent))]).with_style(style().flex_grow().gap(px(2))),
            button(ctx,if ctx.get_state("selected_model")=="" && ctx.get_state("selected_acp")==agent.name {"Selected"} else {"Select"},Fn("acp_select_change"),agent.name,(ctx.get_state("selected_model")!="" || ctx.get_state("selected_acp")!=agent.name) && !ctx.get_state("busy")),
            button(ctx,"Remove",Fn("acp_remove_agent"),agent.name,agents.len()>1 && !ctx.get_state("busy"))
        ]).with_key(`acp-agent-${agent.name}`).with_style(style().padding(px(8)).gap(px(8)).items_center().background(theme_color("surface_raised"))));
    }
    for model in direct_model_list(ctx) {
        rows.push(row([
            column([text(`模型 · ${model.name}`).with_style(style().font_weight(600)),small(`${model.model} · ${model.base_url}`),small(if model.has_api_key {"API Key 已保存"} else {"未设置 API Key"})]).with_style(style().flex_grow().gap(px(2))),
            button(ctx,if ctx.get_state("selected_model")==model.name {"Selected"} else {"Select"},Fn("model_choose"),model.name,ctx.get_state("selected_model")!=model.name),
            button(ctx,"编辑",Fn("model_edit"),model.name,true),button(ctx,"Remove",Fn("model_remove"),model.name,true)
        ]).with_key(`direct-model-${model.name}`).with_style(style().padding(px(8)).gap(px(8)).items_center().background(theme_color("surface_raised"))));
    }
    let fields=if ctx.get_state("registration_type")=="model" {[
        input::Input(#{key:"model-base-url",label:"Base URL",value:ctx.get_state("model_base_url"),placeholder:"https://api.example.com/v1",on_change:Fn("model_base_url_change"),disabled:ctx.get_state("busy")}),
        input::Input(#{key:"model-id",label:"模型名",value:ctx.get_state("model_id"),placeholder:"服务商提供的模型 ID",on_change:Fn("model_id_change"),disabled:ctx.get_state("busy")}),
        input::Input(#{key:"model-api-key",label:"API Key（可选）",value:ctx.get_state("model_api_key"),placeholder:"新建可留空；更新时留空保留已有密钥",on_change:Fn("model_api_key_change"),disabled:ctx.get_state("busy")}),
        small("支持 OpenAI 兼容的 Chat Completions 接口。Base URL 填到 /v1，或填写完整 /chat/completions 地址。")
    ]} else {[
        input::Input(#{key:"acp-agent-command",label:"命令字符串",value:ctx.get_state("acp_new_command"),placeholder:"opencode acp",on_change:Fn("acp_new_command_change"),on_submit:Fn("acp_add_agent"),disabled:ctx.get_state("busy")}),
        small("命令支持参数及引号，例如 opencode acp。")
    ]};
    column([
        watch_timer::WatchTimer(#{key:"watch-timer",sequence:ctx.get_state("watch_tick_id"),on_tick:Fn("watch_tick")}),
        row([button(ctx,"← Back",Fn("acp_page_close"),(),true),text("AI configuration").with_style(style().font_size(px(18)).font_weight(700))]).with_style(style().gap(px(12)).items_center()),
        small("选择 ACP 代理或直连模型，用于生成并提交 Commit message。"),
        if agents.len()==0 && ctx.get_state("projects_loading") {small("Loading saved ACP agents…")} else {small("已注册的 ACP 代理和直连模型")},
        column(rows).with_style(style().gap(px(4))),
        small("手动注册"),
        row([button(ctx,"ACP 代理",Fn("registration_type_change"),"acp",ctx.get_state("registration_type")!="acp"),button(ctx,"直连模型",Fn("registration_type_change"),"model",ctx.get_state("registration_type")!="model")]).with_style(style().gap(px(8))),
        input::Input(#{key:"acp-agent-name",label:"名称",value:ctx.get_state("acp_new_name"),placeholder:"e.g. Xiaoli work",on_change:Fn("acp_new_name_change"),disabled:ctx.get_state("busy")}),
        column(fields).with_style(style().gap(px(12))),
        row([button(ctx,if ctx.get_state("busy") {"保存中…"} else {"注册 / 更新"},if ctx.get_state("registration_type")=="model" {Fn("model_register")} else {Fn("acp_add_agent")},(),!ctx.get_state("busy") && !ctx.get_state("projects_loading"))]).with_style(style().gap(px(8))),
        text(ctx.get_state("acp_message")).with_key("acp-settings-status").with_style(style().font_size(px(13))),
        small("配置自动保存，无需手动编辑配置文件。")
    ]).with_style(style().flex_grow().overflow_y_scroll().padding(px(28)).gap(px(12)).background(theme_color("surface")))
}
fn history_request(ctx,op,extra) {
    if ctx.get_state("busy") || ctx.get_state("selecting") {return;}
    let s=ctx.get_state("snapshot");if s==() {return;}
    watch_invalidate(ctx);
    let data=#{op:op,root:s.root};for key in extra.keys() {data[key]=extra[key];}
    ctx.set_state("busy",true);sync_update_guard(ctx);status(ctx,"Loading Git history…");
    ctx.start_task("alloy.git","execute",data,Fn("history_loaded"),Fn("history_failed"));
}
fn history_failed(ctx,error) {ctx.set_state("busy",false);status(ctx,`${error}`);}
fn history_commits(ctx) {let value=ctx.get_state("history_commits");if value==() {[]} else {value}}
fn history_toggle(ctx,payload) {
    if !guard(ctx) || ctx.get_state("snapshot")==() {return;}
    watch_invalidate(ctx);ctx.set_state("log_mode",!ctx.get_state("log_mode"));
    if ctx.get_state("log_mode") {history_request(ctx,"history",#{skip:0});}
}
fn history_more(ctx,payload) {history_request(ctx,"history",#{skip:history_commits(ctx).len()});}
fn history_select(ctx,id) {
    if ctx.get_state("busy") {return;}
    ctx.set_state("history_selected",id);ctx.set_state("history_details",());ctx.set_state("history_doc",());ctx.set_state("history_file","");
    history_request(ctx,"history_commit",#{commit_id:id});
}
fn history_loaded(ctx,result) {
    ctx.set_state("busy",false);
    if !result.ok {status(ctx,result.message);return;}
    let s=ctx.get_state("snapshot");if s==() || !ctx.get_state("log_mode") || result.history_root!=s.root {return;}
    if result.history_op=="history" {
        let commits=if result.history_skip==0 {[]} else {history_commits(ctx)};
        for commit in result.history.commits {commits.push(commit);}
        ctx.set_state("history_commits",commits);ctx.set_state("history_more",result.history.has_more);
        status(ctx,`${commits.len()} local commits loaded`);
        if result.history_skip==0 {
            ctx.set_state("history_details",());ctx.set_state("history_doc",());ctx.set_state("history_file","");ctx.set_state("history_selected","");
            if commits.len()>0 {history_select(ctx,commits[0].id);}
        }
    } else if result.history_id==ctx.get_state("history_selected") {
        if result.history_op=="history_commit" {ctx.set_state("history_details",result.details);status(ctx,`Commit ${result.details.commit.id.sub_string(0,8)} · ${result.details.files.len()} changed files`);}
        else {ctx.set_state("history_doc",prepare_images(ctx,result.document));status(ctx,`Historical diff: ${result.document.path}`);}
    }
}
fn history_file_select(ctx,path) {if !ctx.get_state("busy") {ctx.set_state("history_file",path);}}
fn history_file_pointer(path,ctx,event) {
    if event.button!="left" {return;}
    history_file_select(ctx,path);
    if event.click_count>=2 {history_show_diff(ctx,path);}
}
fn history_show_diff(ctx,path) {
    if path=="" || ctx.get_state("history_details")==() {return;}
    history_request(ctx,"history_diff",#{commit_id:ctx.get_state("history_selected"),path:path});
}
fn history_diff_selected(ctx,payload) {history_show_diff(ctx,ctx.get_state("history_file"));}
fn history_close_diff(ctx,payload) {if !ctx.get_state("busy") {ctx.set_state("history_doc",());}}
fn history_panel(ctx,s) {
    let rows=[];
    for commit in history_commits(ctx) {
        let commit_color=if ctx.get_state("history_selected")==commit.id {theme_color("selection")} else {theme_color("surface")};
        let commit_style=style().padding_x(px(10)).padding_y(px(5)).gap(px(2)).flex_shrink(false).background(commit_color).hover(style().background(theme_color("surface_hover")));
        let commit_row=column([
            text(commit.subject).with_style(style().font_weight(500).max_height(px(38)).overflow_hidden()),
            small(`${commit.id.sub_string(0,8)}  ·  ${commit.author}  ·  ${commit.date}`)
        ]).with_key(`history-${commit.id}`).accessibility_label(`Commit ${commit.id}: ${commit.subject}`).tab_index(0).on_click_value(Fn("history_select"),commit.id).on_key_value("enter",Fn("history_select"),commit.id).disabled(ctx.get_state("busy"));
        rows.push(commit_row.with_style(commit_style));
    }
    if rows.len()==0 {rows.push(small(if ctx.get_state("busy") {"Loading commits…"} else {"No local commits yet"}));}
    let list=[row([text(`Log · ${s.branch}`).with_style(style().font_weight(600)),button(ctx,"Local Changes",Fn("history_toggle"),(),true)]).with_style(style().padding(px(6)).gap(px(8)).items_center().justify_between()),column(rows).with_key("history-list").with_style(style().flex_grow().min_height(px(0)).overflow_y_scroll())];
    if ctx.get_state("history_more") {list.push(button(ctx,"Load more commits",Fn("history_more"),(),true));}
    let details=ctx.get_state("history_details");let info=[];
    if details==() {info.push(small("Select a commit to view its message and changed files."));}
    else {
        info.push(small(`${details.commit.id} · ${details.commit.author}`));
        info.push(small(details.commit.date));
        info.push(textarea::Textarea(#{key:`history-message-${details.commit.id}`,label:"Commit message",value:details.message,read_only:true,min_rows:2,max_rows:5}));
        info.push(row([small(`${details.files.len()} changed files`),button(ctx,"Show Diff",Fn("history_diff_selected"),(),ctx.get_state("history_file")!="")]).with_style(style().gap(px(8)).items_center().justify_between()));
        if details.commit.parents.len()>1 {info.push(small("Merge commit · compared with first parent"));}
        let files=[];
        for file in details.files {
            let color=if file.status=="A" {theme_color("success")} else if file.status=="D" {theme_color("danger")} else {theme_color("accent")};
            let name=if file.old_path=="" {file.path} else {`${file.old_path} → ${file.path}`};
            let file_style=style().height(px(26)).min_height(px(26)).flex_shrink(false).gap(px(6)).padding_x(px(6)).items_center();
            file_style=file_style.background(if ctx.get_state("history_file")==file.path {theme_color("selection")} else {theme_color("surface")}).hover(style().background(theme_color("surface_hover")));
            let file_row=row([text(file.status).with_style(style().width(px(16)).text_color(color)),text(name).with_style(style().min_width(px(0)).text_ellipsis().overflow_hidden())]);
            file_row=file_row.with_key(`history-file-${file.path}`).accessibility_label(`Show historical diff for ${file.path}`).tab_index(0).on("pointer_up",Fn("history_file_pointer").curry(file.path)).on_key_value("enter",Fn("history_show_diff"),file.path).disabled(ctx.get_state("busy"));
            files.push(file_row.with_style(file_style));
        }
        info.push(column(files).with_key("history-files").with_style(style().flex_grow().min_height(px(0)).overflow_y_scroll()));
    }
    row([
        column(list).with_style(style().flex_grow().min_width(px(0)).min_height(px(0)).border_right(px(1)).border_color(theme_color("border"))),
        column(info).with_key("history-details").with_style(style().width(relative(0.45)).min_width(px(0)).min_height(px(0)).padding(px(8)).gap(px(4)))
    ]).with_key("history-panel").with_style(if ctx.get_state("history_doc")==() {style().flex_grow().min_height(px(0))} else {style().height(px(320)).min_height(px(220)).flex_shrink(false).border_top(px(1)).border_color(theme_color("border"))})
}
fn history_diff_view(ctx) {
    let d=ctx.get_state("history_doc");if d==() {return row([]);}
    let details=ctx.get_state("history_details");
    let left_label=if details.commit.parents.len()==0 {"Empty tree"} else {details.commit.parents[0].sub_string(0,8)};
    let right_label=details.commit.id.sub_string(0,8);
    let old_path="";for file in details.files {if file.path==d.path {old_path=file.old_path;}}
    let nodes=[row([text(`Diff · ${file_name(d.path)}`).with_style(style().flex_grow().min_width(px(0)).text_ellipsis().overflow_hidden()),select::Select(#{key:"history-diff-mode",label:"Diff layout",options:[#{value:"split",label:"Side-by-side viewer"},#{value:"unified",label:"Unified viewer"}],value:ctx.get_state("mode"),open:ctx.get_state("mode_open"),query:"",empty_text:"No diff layouts",size:"xs",width:px(176),on_change:Fn("set_mode"),on_open_change:Fn("mode_open")}),button(ctx,"Close Diff",Fn("history_close_diff"),(),true)]).with_style(style().height(px(34)).flex_shrink(false).padding_x(px(8)).gap(px(8)).items_center().background(theme_color("surface_raised"))),diff_path_bar(ctx,d.path,old_path,"history-diff-path")];
    if d.images!=() {nodes.push(row([image_pane(ctx,`history-left-${right_label}-${d.path}`,d.images.left,left_label,false),image_pane(ctx,`history-right-${right_label}-${d.path}`,d.images.right,right_label,false)]).with_style(style().flex_grow().min_height(px(0)).min_width(px(0))));}
    else if d.note!="" {nodes.push(text(d.note).with_style(style().padding(px(12))));}
    else {nodes.push(diff::DiffViewer(#{key:`history-diff-${right_label}-${d.path}`,left:#{source:d.left,label:left_label,file_name:d.path},right:#{source:d.right,label:right_label,file_name:d.path},mode:ctx.get_state("mode"),context_lines:3,whitespace:"exact",show_hunk_controls:false}).with_style(style().flex_grow().min_height(px(0)).min_width(px(0))));}
    column(nodes).with_key("history-diff").with_style(style().flex_grow().min_height(px(0)).min_width(px(0)))
}
fn history_view(ctx,s) {column([history_diff_view(ctx),history_panel(ctx,s)]).with_key("history-view").with_style(style().flex_grow().min_height(px(0)).min_width(px(0)))}

fn status_metric(key,symbol,count,label,color,with_tooltip) {
    let description=`${label}: ${count}`;
    let metric_style=style().font_family("Menlo").whitespace_nowrap().font_weight(if count>0 {700} else {400}).padding_x(px(4)).padding_y(px(1)).radius(px(3)).border(px(1));
    metric_style=metric_style.text_color(if count>0 {color} else {theme_color("text_muted")}).border_color(if count>0 {color} else {rgba(0x00000000)}).background(if count>0 {theme_color("surface")} else {rgba(0x00000000)});
    let trigger=text(`${symbol}${count}`).accessibility_label(description).with_style(metric_style);
    if !with_tooltip {return trigger.with_key(key).with_style(style().flex_shrink(false));}
    tooltip::Tooltip(#{key:key,label:description,trigger:trigger,content:text(description),placement:"top"}).with_style(style().flex_shrink(false))
}
fn git_metrics(key,p,with_tooltip) {
    let metrics=[
        status_metric(`${key}-changes`,"Δ",p.files,"未提交文件",theme_color("warning"),with_tooltip),
        status_metric(`${key}-added`,"+",p.added,"新增文件",theme_color("success"),with_tooltip),
        status_metric(`${key}-modified`,"~",p.modified,"修改文件",theme_color("accent"),with_tooltip),
        status_metric(`${key}-deleted`,"−",p.deleted,"删除文件",theme_color("danger"),with_tooltip),
        status_metric(`${key}-staged`,"✓",p.staged,"已暂存文件",theme_color("success"),with_tooltip)
    ];
    if p.conflicts>0 {metrics.push(status_metric(`${key}-conflicts`,"!",p.conflicts,"冲突文件",theme_color("danger"),with_tooltip));}
    metrics.push(small("│"));
    if p.upstream=="" {
        let trigger=text("⇅ —").accessibility_label("未设置上游分支").with_style(style().font_family("Menlo").text_color(theme_color("text_muted")));
        metrics.push(if with_tooltip {tooltip::Tooltip(#{key:`${key}-sync-unconfigured`,label:"未设置上游分支",trigger:trigger,content:text("未设置上游分支"),placement:"top"})} else {trigger.with_key(`${key}-sync-unconfigured`)});
    } else {
        metrics.push(status_metric(`${key}-push`,"↑",p.ahead,`待推送提交 · ${p.upstream} · 最近 Fetch 的远程状态`,theme_color("accent"),with_tooltip));
        metrics.push(status_metric(`${key}-pull`,"↓",p.behind,`待拉取提交 · ${p.upstream} · 最近 Fetch 的远程状态`,theme_color("warning"),with_tooltip));
    }
    row(metrics).with_key(key).accessibility_label("项目变更和同步状态").with_style(style().font_size(px(11)).line_height(px(16)).flex_shrink(false).items_center().whitespace_nowrap().gap(px(6)))
}

fn repository_status(ctx, s) {
    if s == () { return row([]); }
    let added=0;let modified=0;let deleted=0;let staged=0;let conflicts=0;
    for file in s.files {
        if (file.index=="?" || file.index=="A") && file.work!="D" {added+=1;}
        if file.index=="D" || file.work=="D" {deleted+=1;}
        if file.index!="?" && file.index!="A" && file.index!="D" && file.work!="D" {modified+=1;}
        if file.index!=" " && file.index!="?" && !file.conflict {staged+=1;}
        if file.conflict {conflicts+=1;}
    }
    git_metrics("repository-status",#{files:s.files.len(),added:added,modified:modified,deleted:deleted,staged:staged,conflicts:conflicts,upstream:s.upstream,ahead:s.ahead,behind:s.behind},true)
}
fn status_bar(ctx,s) {
    row([
        text(if ctx.get_state("selecting") {`Loading ${ctx.get_state("pending_path")}…`} else {ctx.get_state("message")}).with_key("status").with_style(style().font_size(px(11)).flex_grow().min_width(px(0)).max_height(px(60)).overflow_y_scroll().text_color(theme_color("text_muted"))),
        repository_status(ctx,s)
    ]).with_key("status-bar").with_style(style().padding_x(px(10)).padding_y(px(4)).min_height(px(24)).flex_shrink(false).gap(px(12)).items_center().justify_between().background(theme_color("surface_raised")))
}
fn error_banner(ctx) {
    let message=ctx.get_state("operation_error");
    if message=="" {return row([]);}
    row([text(`操作失败：${message}`).with_style(style().flex_grow().min_width(px(0)).max_height(px(140)).overflow_y_scroll().font_weight(600).text_color(theme_color("danger"))),button(ctx,"关闭提示",Fn("dismiss_error"),(),true)])
        .with_key("operation-error").with_style(style().padding(px(12)).gap(px(12)).flex_shrink(false).border(px(1)).border_color(theme_color("danger")).background(theme_color("surface_raised")))
}
fn view(ctx) {
    if ctx.get_state("acp_page_open") {return acp_config_page(ctx);}
    let s = ctx.get_state("snapshot");
    let header = row([
        text("Alloy").with_style(style().font_size(px(14)).font_weight(700)),
        small(if s == () {"A home for your changes"} else {`${s.branch}  ·  ${s.upstream}`}),
        row([
            awake_checkbox(ctx),
            button(ctx,"Log",Fn("history_toggle"),(),s != ()).with_style(style().background(if ctx.get_state("log_mode") {theme_color("selection")} else {theme_color("surface_raised")})),button(ctx,shortcut_label(ctx,"Refresh","alloy.refresh"),Fn("refresh"),(),s != ()),button(ctx,shortcut_label(ctx,"Fetch","alloy.fetch"),Fn("fetch"),(),s != ()),button(ctx,shortcut_label(ctx,"Pull","alloy.pull_prompt"),Fn("pull_prompt"),(),s != ()),button(ctx,shortcut_label(ctx,"Push","alloy.push_prompt"),Fn("push_prompt"),(),s != ()),button(ctx,shortcut_label(ctx,"Tag…","alloy.tag"),Fn("tag_prompt"),(),s != ())
        ]).with_style(style().gap(px(8)).items_center())
    ]).with_style(style().padding_x(px(10)).padding_y(px(4)).gap(px(16)).items_center().justify_between());
    let pathbar = row([
        overlay(text(if s == () {"Projects ▾"} else {`${file_name(s.root)}  ·  ${s.branch}  ▾`}).with_style(style().min_width(px(280)).padding_x(px(10)).padding_y(px(5)).background(theme_color("surface_raised")).hover(style().background(theme_color("surface_hover")))),project_menu(ctx),#{id:"projects-dropdown",kind:"popover",placement:"bottom",open:ctx.get_state("projects_open"),gap:4,modal:false,dismiss_on_escape:true,dismiss_on_outside:true,match_trigger_width:false,initial_focus:"first"}).on_open_change(Fn("projects_toggle")).disabled(ctx.get_state("busy")),
        small(if s == () {"Choose or add a repository"} else {s.root}).with_style(style().flex_grow()),
        button(ctx,"Add project…",Fn("project_add"),(),true)
    ]).with_style(style().padding_x(px(10)).padding_y(px(3)).gap(px(6)).items_center());
    let body = if s == () {
        column([
            text("Make every change intentional.").with_style(style().typography("display_large").font_weight(700)),
            text("Open a repository above, or clone one below.").with_style(style().text_color(theme_color("text_muted"))),
            input::Input(#{key:"clone-url",label:"Clone URL",value:ctx.get_state("url"),placeholder:"https://github.com/owner/repository.git",on_change:Fn("url_change")}),
            input::Input(#{key:"clone-destination",label:"Clone destination",value:ctx.get_state("root"),placeholder:"/absolute/path/to/new-project",on_change:Fn("root_change")}),
            button(ctx,"Clone repository",Fn("clone_repo"),(),true),
            small("Full-context diffs  /  Editable results  /  Three-way conflict resolution")
        ]).with_style(style().flex_grow().justify_center().padding(px(90)).gap(px(20)))
    } else if ctx.get_state("log_mode") {history_view(ctx,s)} else { row(if ctx.get_state("sidebar_visible") {[sidebar(ctx,s),content(ctx)]} else {[content(ctx)]}).with_style(style().flex_grow().min_height(px(0)).min_width(px(0))) };
    column([watch_timer::WatchTimer(#{key:"watch-timer",sequence:ctx.get_state("watch_tick_id"),on_tick:Fn("watch_tick")}),header,pathbar,project_form(ctx),palette_view(ctx),error_banner(ctx),confirmation(ctx),body,
        status_bar(ctx,s)
    ]).with_style(style().font_size(px(13)).line_height(px(18)).width(relative(1)).height(relative(1)).min_width(px(0)).min_height(px(0)).background(theme_color("surface")).text_color(theme_color("text_primary")))
}
