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
    root: string_field(""), url: string_field(""), filter: string_field(""), message: string_field("Opening Alloy…"), commit: string_field(""),
    draft: string_field(""), mode: string_field("split"), tab: string_field("work"), confirm: string_field(""),
    hide_unchanged: bool_field(true), mode_open: bool_field(false), whitespace_open: bool_field(false), whitespace: string_field("exact"),
    open_tabs: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    projects_open: bool_field(false), projects_loading: bool_field(false), add_project: bool_field(false),
    acp_page_open: bool_field(false), acp_select_open: bool_field(false),
    acp_agents: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    selected_acp: string_field("Xiaoli"), acp_new_name: string_field(""), acp_new_command: string_field(""), acp_clear_new_fields: bool_field(false),
    projects: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    project_sessions: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    selecting: bool_field(false), pending_path: string_field(""),
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
    busy: bool_field(false), editor: bool_field(false), base_view: bool_field(false),
    snapshot: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    doc: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } }
} } }
fn dirty(ctx) { let d = ctx.get_state("doc"); d != () && ctx.get_state("draft") != d.right }
fn status(ctx, message) { ctx.set_state("message", message); }
fn failed(ctx, error) { ctx.set_state("busy", false); status(ctx, `${error}`); }
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
fn loaded(ctx, result) {
    if "document" in result { result.document = prepare_images(ctx, result.document); }
    ctx.set_state("busy", false);
    status(ctx, result.message);
    if "committed" in result && result.committed { ctx.set_state("commit",""); }
    if "tag_retry" in result { ctx.set_state("tag",result.tag_retry); ctx.set_state("confirm","tag-retry"); }
    if "overview" in result { let projects = project_list(ctx); let found = false; for i in 0..projects.len() { if projects[i].root == result.overview.root { projects[i] = result.overview; found = true; } } if !found { projects.push(result.overview); } ctx.set_state("projects",projects); }
    if result.snapshot != () {
        let previous_snapshot = ctx.get_state("snapshot");
        let switched = previous_snapshot == () || previous_snapshot.root != result.snapshot.root;
        if switched {
            let sessions = ctx.get_state("project_sessions"); if sessions == () { sessions = #{}; }
            if previous_snapshot != () { sessions[previous_snapshot.root] = #{commit:ctx.get_state("commit"),filter:ctx.get_state("filter")}; }
            let saved = if result.snapshot.root in sessions {sessions[result.snapshot.root]} else {#{commit:"",filter:""}};
            ctx.set_state("commit",saved.commit); ctx.set_state("filter",saved.filter); ctx.set_state("project_sessions",sessions);
            ctx.set_state("included",#{}); ctx.set_state("collapsed",#{}); ctx.set_state("editor",false); ctx.set_state("base_view",false);
        }
        let current = #{}; let mixed = #{};
        if switched {ctx.set_state("open_tabs",[]);}
        for file in result.snapshot.files {
            let staged = file.index != " " && file.index != "?" && !file.conflict;
            current[file.path] = staged && file.work == " ";
            if staged && file.work != " " {mixed[file.path]=true;}
        }
        ctx.set_state("partial",mixed); ctx.set_state("included",current); ctx.set_state("snapshot",result.snapshot);
    }
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
    ctx.set_state("busy", true); status(ctx, `${op}…`);
    ctx.start_task("alloy.git", "execute", data, Fn("loaded"), Fn("failed"));
}
fn watch_invalidate(ctx) {ctx.set_state("watch_epoch",ctx.get_state("watch_epoch")+1);}
fn watch_blocked(ctx) {ctx.get_state("busy") || ctx.get_state("selecting") || dirty(ctx) || ctx.get_state("confirm") != ""}
fn watch_tick(ctx,payload) {
 ctx.set_state("watch_tick_id",ctx.get_state("watch_tick_id")+1);
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
fn push_prompt(ctx,payload) {if guard(ctx) && ctx.get_state("snapshot") != () {ctx.set_state("confirm","push");}}
fn pull_prompt(ctx,payload) {if guard(ctx) && ctx.get_state("snapshot") != () {ctx.set_state("confirm","pull");}}
fn network_confirm(ctx,payload) {let op=ctx.get_state("confirm");if op == "push" || op == "pull" {ctx.set_state("confirm","");network(ctx,op);}}
fn commit_shortcut(ctx,payload) {if ctx.get_state("snapshot") != () && has_message(ctx) && selected_count(ctx)>0 {commit(ctx,());} else {status(ctx,"Stage changes and enter a commit message first.");}}
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
fn cancel(ctx, payload) { ctx.set_state("confirm", ""); ctx.set_state("tag", ()); ctx.set_state("tag_choice", ""); }
fn project_list(ctx) { let value = ctx.get_state("projects"); if value == () {[]} else {value} }
fn projects_loaded(ctx, result) {
    ctx.set_state("projects_loading",false); ctx.set_state("projects",result.projects);
    if "acp_agents" in result {ctx.set_state("acp_agents",result.acp_agents);ctx.set_state("selected_acp",result.selected_acp);}
}
fn projects_failed(ctx, error) { ctx.set_state("projects_loading",false); status(ctx,`Cannot refresh projects: ${error}`); }
fn projects_toggle(ctx, value) {
    ctx.set_state("projects_open",value);
    if ctx.get_state("projects_open") && !ctx.get_state("projects_loading") {
        ctx.set_state("projects_loading",true); ctx.start_task("alloy.git","execute",#{op:"projects"},Fn("projects_loaded"),Fn("projects_failed"));
    }
}
fn acp_page_open(ctx, payload) {
    ctx.set_state("acp_page_open",true);
    ctx.set_state("projects_open",false);
    if !ctx.get_state("projects_loading") {
        ctx.set_state("projects_loading",true); ctx.start_task("alloy.git","execute",#{op:"projects"},Fn("projects_loaded"),Fn("projects_failed"));
    }
}
fn acp_page_close(ctx, payload) {ctx.set_state("acp_page_open",false);}
fn acp_agent_list(ctx) {let agents=ctx.get_state("acp_agents");if agents==() {[]} else {agents}}
fn acp_agent_command(agent) {if "command" in agent && agent.command!="" {agent.command} else {`${agent.path} acp`}}
fn acp_options(ctx) {let options=[];for agent in acp_agent_list(ctx) {options.push(#{value:`agent:${agent.name}`,label:agent.name});}options.push(#{value:"register",label:"手动注册"});options}
fn acp_agent_selector(ctx) {
    if acp_agent_list(ctx).len()==0 {return small("Loading ACP…");}
    select::Select(#{key:"acp-agent-selector",label:"ACP agent",placement:"top",disabled:ctx.get_state("busy"),options:acp_options(ctx),value:`agent:${ctx.get_state("selected_acp")}`,open:ctx.get_state("acp_select_open"),query:"",empty_text:"No ACP agents",size:"xs",width:px(176),on_change:Fn("acp_dropdown_change"),on_open_change:Fn("acp_select_open_changed")})
}
fn acp_dropdown_change(ctx,value) {
    ctx.set_state("acp_select_open",false);
    if value=="register" {acp_page_open(ctx,());return;}
    acp_select_change(ctx,value.sub_string(6));
}
fn acp_select_open_changed(ctx,value) {ctx.set_state("acp_select_open",value);}
fn acp_settings_saved(ctx,result) {
    ctx.set_state("busy",false);ctx.set_state("acp_agents",result.acp_agents);ctx.set_state("selected_acp",result.selected_acp);status(ctx,result.message);
    if ctx.get_state("acp_clear_new_fields") {ctx.set_state("acp_new_name","");ctx.set_state("acp_new_command","");ctx.set_state("acp_clear_new_fields",false);}
}
fn acp_settings_failed(ctx,error) {ctx.set_state("busy",false);ctx.set_state("acp_clear_new_fields",false);status(ctx,`${error}`);}
fn save_acp_settings(ctx,agents,selected) {
    if ctx.get_state("busy") {return;}
    ctx.set_state("busy",true);status(ctx,"Saving ACP settings…");
    ctx.start_task("alloy.git","execute",#{op:"save_acp_settings",acp_agents:agents,selected_acp:selected},Fn("acp_settings_saved"),Fn("acp_settings_failed"));
}
fn acp_select_change(ctx,value) {save_acp_settings(ctx,acp_agent_list(ctx),value);}
fn acp_new_name_change(ctx,value) {ctx.set_state("acp_new_name",value);}
fn acp_new_command_change(ctx,value) {ctx.set_state("acp_new_command",value);}
fn acp_add_agent(ctx,payload) {
    let name=ctx.get_state("acp_new_name").trim();let command=ctx.get_state("acp_new_command").trim();
    if name=="" || command=="" {status(ctx,"请填写名称和命令字符串。");return;}
    let agents=acp_agent_list(ctx);
    for agent in agents {if agent.name==name {status(ctx,`An ACP agent named ${name} already exists.`);return;}}
    agents.push(#{name:name,command:command});ctx.set_state("acp_clear_new_fields",true);
    save_acp_settings(ctx,agents,name);
}
fn acp_remove_agent(ctx,name) {
    let agents=acp_agent_list(ctx);if agents.len()<2 {status(ctx,"Keep at least one ACP agent configured.");return;}
    let remaining=[];for agent in agents {if agent.name!=name {remaining.push(agent);}}
    let selected=ctx.get_state("selected_acp");if selected==name {selected=remaining[0].name;}
    save_acp_settings(ctx,remaining,selected);
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
fn filter_change(ctx, value) { ctx.set_state("filter", value); }
fn commit_change(ctx, value) { ctx.set_state("commit", value); }
fn draft_change(ctx, value) { watch_invalidate(ctx); if !ctx.get_state("selecting") { ctx.set_state("draft", value); } }
fn open_repo(ctx, payload) { if guard(ctx) { request(ctx, "open", #{}); } }
fn browse(ctx, payload) { if guard(ctx) { request(ctx, "browse", #{}); } }
fn clone_repo(ctx, payload) { if guard(ctx) { request(ctx, "clone", #{url:ctx.get_state("url")}); } }
fn refresh(ctx, payload) { if guard(ctx) && ctx.get_state("snapshot") != () { request(ctx, "refresh", #{}); } }
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
    if ctx.get_state("busy") { return; }
    if dirty(ctx) { status(ctx, "You have an unsaved draft. Save it or discard it before changing context."); return; }
    let d = ctx.get_state("doc");
    if !ctx.get_state("selecting") && d != () && d.path == path && d.staged == staged { return; }
    let old_path = "";
    for file in ctx.get_state("snapshot").files { if file.path == path { old_path = file.old_path; } }
    let id = ctx.get_state("selection_id") + 1;
    ctx.set_state("selection_id", id); ctx.set_state("pending_path", path); ctx.set_state("selecting", true);
    ctx.start_task("alloy.git", "execute", #{op:"select", root:ctx.get_state("snapshot").root, path:path, old_path:old_path, staged:staged, selection_id:id}, Fn("selected"), Fn("selection_failed"));
}
fn select(ctx, path) { select_file(ctx, path, ctx.get_state("tab") == "staged"); }
fn set_tab(ctx, value) { let d = ctx.get_state("doc"); if d != () { select_file(ctx, d.path, value == "staged"); } }
fn opened(ctx) { let tabs = ctx.get_state("open_tabs"); if tabs == () {[]} else {tabs} }
fn close_tab(ctx, path) {
    watch_invalidate(ctx);
    let d = ctx.get_state("doc");
    if ctx.get_state("selecting") || ctx.get_state("busy") { return; }
    if d != () && d.path == path && !guard(ctx) { return; }
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
    groups[group] = !(group in groups && groups[group]); ctx.set_state("collapsed", groups);
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
fn accept_file(ctx, side) { let d = ctx.get_state("doc"); ctx.set_state("draft", if side == "ours" {d.ours} else {d.theirs}); ctx.set_state("editor",true); status(ctx,"Replaced the draft with the selected side. Review before marking resolved."); }
fn small(value) { text(value).with_style(style().typography("caption").text_color(theme_color("text_muted"))) }
fn button(ctx, label, handler, payload, enabled) {
    text(label).with_style(style().font_size(px(12)).line_height(px(18)).padding_x(px(7)).padding_y(px(3)).radius(px(3))
        .background(theme_color("surface_raised"))
        .hover(style().background(theme_color("surface_hover"))).disabled(style().opacity(0.4))).disabled(!enabled || ctx.get_state("busy"))
        .accessibility_role("button").accessibility_label(label).on_click_value(handler,payload)
}
fn tab(ctx, label, value) {
    button(ctx,label,Fn("set_tab"),value,true).with_style(style().background(if ctx.get_state("tab") == value {theme_color("selection")} else {theme_color("surface_raised")}))
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
    if c == "tag" { return tag_bump_bar(ctx); }
    if c == "tag-go" { return tag_go_bar(ctx); }
    if c == "tag-retry" { return tag_retry_bar(ctx); }
    let label = if c == "push" {"Push local commits? If no upstream is configured, automatically set it on the selected push remote."} else if c == "pull" {"Pull and merge changes from the configured upstream?"} else if c == "close" {"Close Alloy and discard unsaved work? An in-flight Git operation may still finish."} else if c == "delete" {"Delete this conflicted file and stage the deletion?"} else {"Discard your unsaved draft?"};
    let handler = if c == "push" || c == "pull" {Fn("network_confirm")} else if c == "close" {Fn("close_now")} else if c == "delete" {Fn("delete_now")} else {Fn("discard_now")};
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
fn sidebar(ctx,s) {
    let items = []; let checked = inclusion(ctx); let partial = partials(ctx); let groups = ctx.get_state("collapsed");
    let filter = ctx.get_state("filter").to_lower();
    for group in ["Changes", "Unversioned Files", "Merge Conflicts"] {
        let files = []; let paths = []; let count = 0; let mixed = false;
        for file in s.files {
            if file_group(file) != group || (filter != "" && !file.path.to_lower().contains(filter)) { continue; }
            files.push(file); paths.push(file.path);
            if file.path in checked && checked[file.path] { count += 1; }
            if file.path in partial { mixed = true; }
        }
        if files.len() == 0 { continue; }
        let folded = groups != () && group in groups && groups[group];
        let title = row([text(if folded {"›"} else {"⌄"}),text(group).with_style(style().font_weight(600)),small(`${files.len()} files`)])
            .with_style(style().flex_grow().gap(px(6)).items_center()).on_click_value(Fn("collapse_group"),group);
        items.push(row([check_box(ctx,`Stage ${group}`,count == files.len() && !mixed,mixed || (count > 0 && count < files.len()),Fn("toggle_group"),paths,group != "Merge Conflicts" && !s.merging),title])
            .with_key(`group-${group}`).with_style(style().height(px(28)).min_height(px(28)).flex_shrink(false).padding_x(px(6)).gap(px(6)).items_center()));
        if folded { continue; }
        for f in files {
            let selected = if ctx.get_state("selecting") {ctx.get_state("pending_path") == f.path} else {ctx.get_state("doc") != () && ctx.get_state("doc").path == f.path};
            let color = if f.conflict {theme_color("danger")} else if f.index == "?" || f.index == "A" {theme_color("success")} else {theme_color("accent")};
            let parts = f.path.split("/"); let name = parts[parts.len()-1];
            let directory = if parts.len() > 1 { f.path.sub_string(0, f.path.len()-name.len()-1) } else {""};
            let status = if f.conflict {"!"} else if f.index == "?" {"?"} else if f.index != " " {f.index} else {f.work};
            let label = row([
                text(status).with_style(style().width(px(14)).flex_shrink(false).font_family("Menlo").font_size(px(11)).text_center().text_color(color)),
                text(name).with_style(style().flex_shrink(false).text_color(color)),
                text(directory).with_style(style().text_color(theme_color("text_muted")).min_width(px(0)).text_ellipsis().overflow_hidden())
            ]).with_style(style().height(relative(1)).flex_grow().min_width(px(0)).gap(px(5)).items_center().whitespace_nowrap().overflow_hidden())
                .accessibility_label(f.path).on_click_value(Fn("select"),f.path).disabled(ctx.get_state("busy"));
            let row_style = style().height(px(26)).min_height(px(26)).flex_shrink(false).padding_left(px(22)).padding_right(px(6)).gap(px(6)).items_center();
            let row_style = row_style.radius(px(3)).background(if selected {theme_color("selection")} else {theme_color("surface_raised")}).hover(style().background(theme_color("surface_hover")));
            items.push(row([check_box(ctx,`Stage ${f.path}`,f.path in checked && checked[f.path],f.path in partial,Fn("toggle_include"),f.path,!f.conflict && !s.merging),label]).with_key(`file-${f.path}`).with_style(row_style));
        }
    }
    let can_commit = has_message(ctx) && selected_count(ctx) > 0;
    let footer = [textarea::Textarea(#{key:`commit-${ctx.get_state("focus_commit_id")}`,autofocus:ctx.get_state("focus_commit_id")>0,label:"Commit message",value:ctx.get_state("commit"),placeholder:"Commit message…",rows:3,on_change:Fn("commit_change")})];
    if s.merging { footer.push(button(ctx,"Commit merge (staged)",Fn("commit_index"),(),has_message(ctx))); }
    else { footer.push(button(ctx,`Commit ${selected_count(ctx)} staged files`,Fn("commit"),(),can_commit)); }
    if ctx.get_state("advanced") && !s.merging { footer.push(button(ctx,"Commit staged index",Fn("commit_index"),(),has_message(ctx))); }
    footer.push(row([button(ctx,shortcut_label(ctx,"Commit","alloy.ai_commit"),Fn("ai_commit"),(),true),acp_agent_selector(ctx)]).with_style(style().gap(px(8)).items_center()));
    column([
        row([text("Commit").with_style(style().font_size(px(14)).font_weight(600)),button(ctx,shortcut_label(ctx,"Refresh","alloy.refresh"),Fn("refresh"),(),true),button(ctx,if ctx.get_state("advanced") {"Hide staging"} else {"Staging…"},Fn("staging_toggle"),(),true)]).with_style(style().padding_x(px(6)).gap(px(6)).items_center()),
        input::Input(#{key:`filter-${ctx.get_state("focus_filter_id")}`,autofocus:ctx.get_state("focus_filter_id")>0,label:"Filter changed files",value:ctx.get_state("filter"),placeholder:"Filter files…",on_change:Fn("filter_change"),part_styles:#{input:style().height(px(26)).font_size(px(12))}}),
        column(items).with_key("file-list").with_style(style().flex_grow().min_height(px(0)).overflow_y_scroll()),
        column(footer).with_style(style().border_top(px(1)).border_color(theme_color("border")).padding_top(px(6)).gap(px(5)))
    ]).with_style(style().width(px(350)).flex_shrink(false).padding(px(6)).gap(px(4)).border_right(px(1)).border_color(theme_color("border")).background(theme_color("surface_raised")))
}
fn content(ctx) {
    let d = ctx.get_state("doc");
    if d == () {
        return column([text("Select a file to review").with_style(style().typography("display")),small("Choose a changed file on the left. File changes refresh automatically.")])
            .with_style(style().flex_grow().items_center().justify_center().gap(px(12)));
    }
    let toolbar = [
        button(ctx,"←",Fn("previous_file"),(),true), button(ctx,"→",Fn("next_file"),(),true),
        button(ctx,if ctx.get_state("editor") {"Hide editor"} else {"Edit"},Fn("edit"),(),d.editable),
        select::Select(#{key:"viewer-mode",label:"Diff layout",options:[#{value:"split",label:"Side-by-side viewer"},#{value:"unified",label:"Unified viewer"}],value:ctx.get_state("mode"),open:ctx.get_state("mode_open"),query:"",empty_text:"No options",size:"xs",width:px(176),on_change:Fn("set_mode"),on_open_change:Fn("mode_open")}),
        select::Select(#{key:"whitespace",label:"Whitespace comparison",options:[#{value:"exact",label:"Do not ignore"},#{value:"ignore_changes",label:"Ignore changes"},#{value:"ignore_all",label:"Ignore all spaces"}],value:ctx.get_state("whitespace"),open:ctx.get_state("whitespace_open"),query:"",empty_text:"No options",size:"xs",width:px(156),on_change:Fn("whitespace_change"),on_open_change:Fn("whitespace_open")})
    ];
    toolbar.push(button(ctx,if ctx.get_state("hide_unchanged") {"Show all lines"} else {"Hide unchanged"},Fn("toggle_unchanged"),(),true));
    if d.images != () { toolbar = [button(ctx,"←",Fn("previous_file"),(),true),button(ctx,"→",Fn("next_file"),(),true),small("Image preview"),button(ctx,if ctx.get_state("image_actual") {"Reduced size"} else {"Actual size (100%)"},Fn("image_zoom"),(),true)]; }
    if dirty(ctx) { toolbar.push(button(ctx,"Save",Fn("save"),(),d.editable)); toolbar.push(button(ctx,"Discard",Fn("discard"),(),true)); }
    if ctx.get_state("advanced") {
        toolbar.push(tab(ctx,"Local","work")); toolbar.push(tab(ctx,"Staged","staged"));
        toolbar.push(button(ctx,"Stage",Fn("stage"),(),!d.conflict)); toolbar.push(button(ctx,"Unstage",Fn("unstage"),(),!d.conflict));
    }
    let children = [document_tabs(ctx,d),row(toolbar).with_style(style().height(px(34)).flex_shrink(false).gap(px(4)).padding_x(px(6)).items_center().overflow_x_scroll().background(theme_color("surface_raised")))];
    if d.images != () {
        let panes = [];
        if !d.added { panes.push(image_pane(ctx,`image-left-${d.path}`,d.images.left,"HEAD",false)); }
        panes.push(image_pane(ctx,`image-right-${d.path}`,d.images.right,if d.staged {"Staged version"} else {"Current version"},d.added));
        children.push(row(panes).with_style(style().flex_grow().min_width(px(0)).min_height(px(0)).gap(px(1))));
    }
    else if d.note != "" { children.push(text(d.note).with_style(style().padding(px(24)).text_color(theme_color("warning")))); }
    else if d.conflict {
        children.push(row([small("CONFLICT · Compare both sides, edit the result, then mark resolved."),button(ctx,"Base",Fn("base_toggle"),(),true),button(ctx,"Mark resolved",Fn("resolve"),(),true),button(ctx,"Resolve as deletion",Fn("delete_prompt"),(),true)]).with_style(style().gap(px(6)).items_center()));
        children.push(viewer(ctx,`conflict-${d.path}`,d.ours,d.theirs,"OURS · index stage 2","THEIRS · index stage 3",d.path,"split"));
        if ctx.get_state("base_view") {
            children.push(code::CodeViewer(#{key:`base-${d.path}`,source:d.base,file_name:d.path,label:"COMMON ANCESTOR"}).with_style(style().height(px(160))));
        }
        let actions = [button(ctx,"Use entire ours",Fn("accept_file"),"ours",true),button(ctx,"Use entire theirs",Fn("accept_file"),"theirs",true)];
        let number = 0;
        for line in ctx.get_state("draft").split("\n") {
            if line.starts_with("<<<<<<<") {
                actions.push(small(`Block ${number+1}`));
                for choice in ["ours","theirs","both"] { actions.push(button(ctx,choice,Fn("choose"),#{number:number,choice:choice},true)); }
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
    if ctx.get_state("editor") && d.editable { children.push(editor(ctx,d)); }
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
    let nodes = [small(if ctx.get_state("projects_loading") {"Refreshing project summaries…"} else {"Recent projects · incoming/outgoing commits reflect the last Fetch"}).with_style(style().padding_x(px(10)))];
    let s = ctx.get_state("snapshot");
    for p in project_list(ctx) {
        let active = s != () && s.root == p.root;
        let counts = if p.error != "" {"Repository unavailable · choose to retry"} else {
            `${p.files} changed files / to commit  ·  +${p.added} added  ~${p.modified} modified  −${p.deleted} deleted  ·  ${p.staged} staged  ·  ${p.conflicts} conflicts  ·  ` + if p.upstream == "" {"No upstream"} else {`↑ ${p.ahead} to push   ↓ ${p.behind} to pull`}
        };
        nodes.push(column([
            row([text(if active {`✓ ${p.name}`} else {p.name}).with_style(style().font_weight(600)),small(p.branch),small(p.root)]).with_style(style().gap(px(12)).items_center()),
            small(counts)
        ]).with_key(`project-${p.root}`).with_style(style().padding_x(px(12)).padding_y(px(6)).gap(px(2)).background(if active {theme_color("selection")} else {theme_color("surface_raised")}).hover(style().background(theme_color("surface_hover"))))
        .tab_index(0).on_key_value("enter",Fn("project_choose"),p.root).accessibility_label(`Open project ${p.root}`).on_click_value(Fn("project_choose"),p.root).disabled(ctx.get_state("busy") || ctx.get_state("selecting")));
    }
    if project_list(ctx).len() == 0 && !ctx.get_state("projects_loading") { nodes.push(small("No recent projects. Add a repository to begin.")); }
    column(nodes).with_key("project-menu").with_style(style().width(px(960)).max_height(px(360)).overflow_y_scroll().flex_shrink(false).padding(px(6)).gap(px(3)).background(theme_color("surface_raised")))
}
fn acp_config_page(ctx) {
    let agents=acp_agent_list(ctx);let rows=[];
    for agent in agents {
        rows.push(row([
            column([text(agent.name).with_style(style().font_weight(600)),small(acp_agent_command(agent))]).with_style(style().flex_grow().gap(px(2))),
            button(ctx,if ctx.get_state("selected_acp")==agent.name {"Selected"} else {"Select"},Fn("acp_select_change"),agent.name,ctx.get_state("selected_acp")!=agent.name && !ctx.get_state("busy")),
            button(ctx,"Remove",Fn("acp_remove_agent"),agent.name,agents.len()>1 && !ctx.get_state("busy"))
        ]).with_key(`acp-agent-${agent.name}`).with_style(style().padding(px(8)).gap(px(8)).items_center().background(theme_color("surface_raised"))));
    }
    column([
        row([button(ctx,"← Back",Fn("acp_page_close"),(),true),text("ACP configuration").with_style(style().font_size(px(18)).font_weight(700))]).with_style(style().gap(px(12)).items_center()),
        small("配置用于生成提交信息的 ACP 代理。"),
        if agents.len()==0 && ctx.get_state("projects_loading") {small("Loading saved ACP agents…")} else {small("Configured agents")},
        column(rows).with_style(style().gap(px(4))),
        small("手动注册"),
        input::Input(#{key:"acp-agent-name",label:"名称",value:ctx.get_state("acp_new_name"),placeholder:"e.g. Xiaoli work",on_change:Fn("acp_new_name_change"),disabled:ctx.get_state("busy")}),
        input::Input(#{key:"acp-agent-command",label:"命令字符串",value:ctx.get_state("acp_new_command"),placeholder:"opencode acp",on_change:Fn("acp_new_command_change"),disabled:ctx.get_state("busy")}),
        row([button(ctx,"注册",Fn("acp_add_agent"),(),!ctx.get_state("busy") && !ctx.get_state("projects_loading"))]).with_style(style().gap(px(8))),
        small("命令支持参数及引号，例如 opencode acp 或 \"/path with spaces/agent\" acp。配置自动保存。")
    ]).with_style(style().flex_grow().padding(px(28)).gap(px(12)).background(theme_color("surface")))
}
fn view(ctx) {
    if ctx.get_state("acp_page_open") {return acp_config_page(ctx);}
    let s = ctx.get_state("snapshot");
    let header = row([
        text("Alloy").with_style(style().font_size(px(14)).font_weight(700)),
        small(if s == () {"A home for your changes"} else {`${s.branch}  ·  ↑${s.ahead} ↓${s.behind}  ${s.upstream}`}),
        row([
            button(ctx,shortcut_label(ctx,"Refresh","alloy.refresh"),Fn("refresh"),(),s != ()),button(ctx,shortcut_label(ctx,"Fetch","alloy.fetch"),Fn("fetch"),(),s != ()),button(ctx,shortcut_label(ctx,"Pull","alloy.pull_prompt"),Fn("pull_prompt"),(),s != ()),button(ctx,shortcut_label(ctx,"Push","alloy.push_prompt"),Fn("push_prompt"),(),s != ()),button(ctx,shortcut_label(ctx,"Tag…","alloy.tag"),Fn("tag_prompt"),(),s != ())
        ]).with_style(style().gap(px(8)))
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
    } else { row(if ctx.get_state("sidebar_visible") {[sidebar(ctx,s),content(ctx)]} else {[content(ctx)]}).with_style(style().flex_grow().min_height(px(0)).min_width(px(0))) };
    column([watch_timer::WatchTimer(#{key:"watch-timer",sequence:ctx.get_state("watch_tick_id"),on_tick:Fn("watch_tick")}),header,pathbar,project_form(ctx),palette_view(ctx),confirmation(ctx),body,
        text(if ctx.get_state("selecting") {`Loading ${ctx.get_state("pending_path")}…`} else {ctx.get_state("message")}).with_key("status").with_style(style().font_size(px(11)).padding_x(px(10)).padding_y(px(3)).min_height(px(24)).max_height(px(60)).overflow_y_scroll().background(theme_color("surface_raised")).text_color(theme_color("text_muted")))
    ]).with_style(style().font_size(px(13)).line_height(px(18)).width(relative(1)).height(relative(1)).min_width(px(0)).min_height(px(0)).background(theme_color("surface")).text_color(theme_color("text_primary")))
}
