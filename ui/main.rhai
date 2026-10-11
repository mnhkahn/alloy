import "components/input" as input;
import "components/textarea" as textarea;
import "components/diff_viewer" as diff;
import "components/code_viewer" as code;

fn string_field(value) { #{ schema: #{ type: "string" }, "default": #{ type: "string", value: value } } }
fn bool_field(value) { #{ schema: #{ type: "bool" }, "default": #{ type: "bool", value: value } } }
fn state_schema() { #{ fields: #{
    root: string_field(""), url: string_field(""), filter: string_field(""), message: string_field("Opening Alloy…"), commit: string_field(""),
    draft: string_field(""), mode: string_field("split"), tab: string_field("work"), confirm: string_field(""),
    busy: bool_field(false), editor: bool_field(false), base_view: bool_field(false),
    snapshot: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } },
    doc: #{ schema: #{ type: "ui_value" }, "default": #{ type: "null" } }
} } }
fn dirty(ctx) { let d = ctx.get_state("doc"); d != () && ctx.get_state("draft") != d.right }
fn status(ctx, message) { ctx.set_state("message", message); }
fn failed(ctx, error) { ctx.set_state("busy", false); status(ctx, `${error}`); }
fn loaded(ctx, result) {
    ctx.set_state("busy", false);
    status(ctx, result.message);
    if result.snapshot != () { ctx.set_state("snapshot", result.snapshot); }
    if !result.ok {
        if !dirty(ctx) && result.document != () {
            ctx.set_state("doc", result.document); ctx.set_state("draft", result.document.right);
            ctx.set_state("editor", result.document.conflict); ctx.set_state("tab", "work");
        }
        return;
    }
    if "draft" in result { ctx.set_state("draft", result.draft); ctx.set_state("editor", true); return; }
    let previous = ctx.get_state("doc");
    ctx.set_state("doc", result.document);
    ctx.set_state("confirm", "");
    if result.snapshot != () { ctx.set_state("root", result.snapshot.root); }
    if result.document != () { ctx.set_state("draft", result.document.right); if result.document.conflict { ctx.set_state("tab", "work"); } if previous == () || previous.path != result.document.path { ctx.set_state("editor", result.document.conflict); } }
    else { ctx.set_state("draft", ""); }
}
fn request(ctx, op, extra) {
    if ctx.get_state("busy") { return; }
    let d = ctx.get_state("doc");
    let active = ctx.get_state("snapshot");
    let repo_root = if op == "open" || op == "clone" || active == () { ctx.get_state("root") } else { active.root };
    let data = #{ op: op, root: repo_root, path: if d == () { "" } else { d.path }, staged: ctx.get_state("tab") == "staged" };
    for key in extra.keys() { data[key] = extra[key]; }
    ctx.set_state("busy", true); status(ctx, `${op}…`);
    ctx.start_task("alloy.git", "execute", data, Fn("loaded"), Fn("failed"));
}
fn guard(ctx) {
    if ctx.get_state("busy") { return false; }
    if dirty(ctx) { status(ctx, "You have an unsaved draft. Save it or discard it before changing context."); return false; }
    true
}
fn init(ctx) {
    ctx.register_action("alloy.save", Fn("save")); ctx.register_action("alloy.refresh", Fn("refresh")); ctx.register_action("alloy.open", Fn("browse"));
    ctx.set_close_handler(Fn("closing"));
    let fixture = ctx.call_capability("alloy.boot", "state", ());
    if fixture == () { request(ctx, "bootstrap", #{}); } else { loaded(ctx, fixture); }
}
fn closing(ctx, payload) {
    if dirty(ctx) || ctx.get_state("busy") { ctx.set_state("confirm", "close"); }
    else { ctx.close_window(ctx.window_id()); }
}
fn close_now(ctx, payload) { ctx.close_window(ctx.window_id()); }
fn cancel(ctx, payload) { ctx.set_state("confirm", ""); }
fn root_change(ctx, value) { ctx.set_state("root", value); }
fn url_change(ctx, value) { ctx.set_state("url", value); }
fn filter_change(ctx, value) { ctx.set_state("filter", value); }
fn commit_change(ctx, value) { ctx.set_state("commit", value); }
fn draft_change(ctx, value) { ctx.set_state("draft", value); }
fn open_repo(ctx, payload) { if guard(ctx) { request(ctx, "open", #{}); } }
fn browse(ctx, payload) { if guard(ctx) { request(ctx, "browse", #{}); } }
fn clone_repo(ctx, payload) { if guard(ctx) { request(ctx, "clone", #{url:ctx.get_state("url")}); } }
fn refresh(ctx, payload) { if guard(ctx) && ctx.get_state("snapshot") != () { request(ctx, "refresh", #{}); } }
fn select(ctx, path) { if guard(ctx) { request(ctx, "select", #{path:path}); } }
fn set_tab(ctx, value) { if guard(ctx) { ctx.set_state("tab", value); request(ctx, "select", #{}); } }
fn set_mode(ctx, value) { ctx.set_state("mode", value); }
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
fn network(ctx, op) { if guard(ctx) { request(ctx, op, #{}); } }
fn commit(ctx, payload) { if guard(ctx) { request(ctx, "commit", #{message:ctx.get_state("commit")}); } }
fn restore(ctx, number) { request(ctx, "hunk", #{left:ctx.get_state("doc").left,content:ctx.get_state("draft"),number:number}); }
fn choose(ctx, data) { request(ctx, "choose", #{content:ctx.get_state("draft"),number:data.number,choice:data.choice}); }
fn accept_file(ctx, side) { let d = ctx.get_state("doc"); ctx.set_state("draft", if side == "ours" {d.ours} else {d.theirs}); ctx.set_state("editor",true); status(ctx,"Replaced the draft with the selected side. Review before marking resolved."); }
fn small(value) { text(value).with_style(style().typography("caption").text_color(theme_color("text_muted"))) }
fn button(ctx, label, handler, payload, enabled) {
    text(label).with_style(style().padding_x(px(10)).padding_y(px(7)).radius(px(5))
        .background(theme_color("surface_raised")).border(px(1)).border_color(theme_color("border"))
        .hover(style().background(theme_color("surface_hover"))).disabled(style().opacity(0.4))).disabled(!enabled || ctx.get_state("busy"))
        .accessibility_role("button").accessibility_label(label).on_click_value(handler,payload)
}
fn tab(ctx, label, value) {
    button(ctx,label,Fn("set_tab"),value,true).with_style(style().background(if ctx.get_state("tab") == value {theme_color("selection")} else {theme_color("surface_raised")}))
}
fn viewer(key,left,right,left_label,right_label,file,mode) {
    diff::DiffViewer(#{key:key,left:#{source:left,label:left_label,file_name:file},right:#{source:right,label:right_label,file_name:file},mode:mode,context_lines:"all"})
        .with_style(style().flex_grow().min_height(px(0)).min_width(px(0)))
}
fn editor(ctx,d) {
    column([
        row([text(if d.conflict {"MERGE RESULT"} else {"WORKING TREE · EDIT"}).with_style(style().font_weight(600)),small(if dirty(ctx) {"● Unsaved draft"} else {"Saved"})]).with_style(style().justify_between()),
        textarea::Textarea(#{ key:`editor-${d.path}`, label:"Editable file content", value:ctx.get_state("draft"), rows:15, on_change:Fn("draft_change"), disabled:ctx.get_state("busy"), part_styles:#{editor:style().font_family("Menlo").font_size(px(12))} })
    ]).with_style(style().gap(px(8)).padding(px(10)).background(theme_color("surface_raised")))
}
fn confirmation(ctx) {
    let c = ctx.get_state("confirm");
    if c == "" { return row([]); }
    let label = if c == "close" {"Close Alloy and discard unsaved work? An in-flight Git operation may still finish."} else if c == "delete" {"Delete this conflicted file and stage the deletion?"} else {"Discard your unsaved draft?"};
    let handler = if c == "close" {Fn("close_now")} else if c == "delete" {Fn("delete_now")} else {Fn("discard_now")};
    row([text(label),button(ctx,"Confirm",handler,(),true),button(ctx,"Cancel",Fn("cancel"),(),true)])
        .with_style(style().padding(px(12)).gap(px(10)).items_center().background(theme_color("selection")))
}
fn has_message(ctx) { let value = ctx.get_state("commit"); value.trim(); value != "" }
fn sidebar(ctx,s) {
    let items = [];
    let filter = ctx.get_state("filter").to_lower();
    for f in s.files {
        if filter != "" && !f.path.to_lower().contains(filter) { continue; }
        let selected = ctx.get_state("doc") != () && ctx.get_state("doc").path == f.path;
        let color = if f.conflict {theme_color("danger")} else if f.index == "?" || f.index == "A" {theme_color("success")} else {theme_color("accent")};
        items.push(column([
            text(`${f.index}${f.work}  ${f.path}`).with_style(style().text_color(color)),
            small(if f.conflict {"CONFLICT"} else if f.old_path != "" {`from ${f.old_path}`} else {if f.index != " " && f.index != "?" {"Index changes"} else {"Working tree changes"}})
        ]).with_key(`file-${f.path}`).with_style(style().padding(px(10)).gap(px(4)).radius(px(4)).background(if selected {theme_color("selection")} else {theme_color("surface_raised")}).hover(style().background(theme_color("surface_hover"))))
            .on_click_value(Fn("select"), f.path).disabled(ctx.get_state("busy")));
    }
    column([
        row([text("CHANGES").with_style(style().font_weight(700)),small(`${s.files.len()} files`)]).with_style(style().justify_between()),
        input::Input(#{key:"filter",label:"Filter changed files",value:ctx.get_state("filter"),placeholder:"Filter files…",on_change:Fn("filter_change")}),
        column(items).with_key("file-list").with_style(style().flex_grow().min_height(px(0)).overflow_y_scroll().gap(px(4))),
        small("COMMIT STAGED CHANGES"),
        textarea::Textarea(#{key:"commit",label:"Commit message",value:ctx.get_state("commit"),placeholder:"Describe your change…",rows:3,on_change:Fn("commit_change")}),
        button(ctx,if s.merging {"Commit merge"} else {"Commit staged"},Fn("commit"),(),has_message(ctx)),
        small("⌘S Save  ·  ⌘R Refresh  ·  ⌘O Open")
    ]).with_style(style().width(px(290)).flex_shrink(false).padding(px(14)).gap(px(12)).background(theme_color("surface_raised")))
}
fn content(ctx) {
    let d = ctx.get_state("doc");
    if d == () {
        return column([text("Working tree is clean").with_style(style().typography("display")),small("Your next change will appear here. Press ⌘R to refresh.")])
            .with_style(style().flex_grow().items_center().justify_center().gap(px(12)));
    }
    let children = [row([
        text(d.path).with_style(style().font_family("Menlo").font_weight(600)),
        row([button(ctx,"Stage file",Fn("stage"),(),!d.conflict),button(ctx,"Unstage",Fn("unstage"),(),!d.conflict)]) .with_style(style().gap(px(6)))
    ]).with_style(style().justify_between().items_center())];
    children.push(row([
        tab(ctx,"Working tree","work"),tab(ctx,"Staged","staged"),
        button(ctx,"Split",Fn("set_mode"),"split",true),button(ctx,"Unified",Fn("set_mode"),"unified",true),
        button(ctx,if ctx.get_state("editor") {"Hide editor"} else {"Edit result"},Fn("edit"),(),d.editable),
        button(ctx,"Save  ⌘S",Fn("save"),(),d.editable && dirty(ctx)),button(ctx,"Discard draft",Fn("discard"),(),dirty(ctx))
    ]).with_style(style().gap(px(6)).items_center()));
    if d.note != "" { children.push(text(d.note).with_style(style().padding(px(24)).text_color(theme_color("warning")))); }
    else if d.conflict {
        children.push(row([small("CONFLICT · Compare both sides, edit the result, then mark resolved."),button(ctx,"Base",Fn("base_toggle"),(),true),button(ctx,"Mark resolved",Fn("resolve"),(),true),button(ctx,"Resolve as deletion",Fn("delete_prompt"),(),true)]).with_style(style().gap(px(6)).items_center()));
        children.push(viewer(`conflict-${d.path}`,d.ours,d.theirs,"OURS · index stage 2","THEIRS · index stage 3",d.path,"split"));
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
        children.push(row(actions).with_key("conflict-actions").with_style(style().gap(px(6)).overflow_x_scroll().min_height(px(38))));
    } else {
        children.push(viewer(`diff-${d.path}-${d.staged}`,d.left,ctx.get_state("draft"),if d.staged {"HEAD"} else {"INDEX"},if d.staged {"INDEX"} else {"WORKING TREE"},d.path,ctx.get_state("mode")));
        let actions = [];
        if !d.staged && !dirty(ctx) {
            let i = 0; for h in d.hunks { actions.push(button(ctx,`↶ ${h.label}`,Fn("restore"),i,true)); i += 1; }
        }
        if actions.len() > 0 { children.push(row(actions).with_key("hunk-actions").with_style(style().gap(px(6)).overflow_x_scroll().min_height(px(38)))); }
    }
    if ctx.get_state("editor") && d.editable { children.push(editor(ctx,d)); }
    column(children).with_style(style().flex_grow().min_width(px(0)).min_height(px(0)).padding(px(14)).gap(px(10)))
}
fn view(ctx) {
    let s = ctx.get_state("snapshot");
    let header = row([
        text("Alloy").with_style(style().typography("display").font_weight(700)),
        small(if s == () {"A home for your changes"} else {`${s.branch}  ·  ↑${s.ahead} ↓${s.behind}  ${s.upstream}`}),
        row([button(ctx,"Refresh",Fn("refresh"),(),s != ()),button(ctx,"Fetch",Fn("network"),"fetch",s != ()),button(ctx,"Pull",Fn("network"),"pull",s != ()),button(ctx,"Push",Fn("network"),"push",s != ())]).with_style(style().gap(px(8)))
    ]).with_style(style().padding_x(px(20)).padding_y(px(14)).gap(px(22)).items_center().justify_between());
    let pathbar = row([
        input::Input(#{key:"repository",label:"Repository path",value:ctx.get_state("root"),placeholder:"/path/to/repository",on_change:Fn("root_change"),on_submit:Fn("open_repo"),disabled:ctx.get_state("busy")}).with_style(style().flex_grow()),
        button(ctx,"Open",Fn("open_repo"),(),true),button(ctx,"Browse…",Fn("browse"),(),true)
    ]).with_style(style().padding_x(px(20)).padding_y(px(8)).gap(px(8)));
    let body = if s == () {
        column([
            text("Make every change intentional.").with_style(style().typography("display_large").font_weight(700)),
            text("Open a repository above, or clone one below.").with_style(style().text_color(theme_color("text_muted"))),
            input::Input(#{key:"clone-url",label:"Clone URL",value:ctx.get_state("url"),placeholder:"https://github.com/owner/repository.git",on_change:Fn("url_change")}),
            small("Enter a new absolute destination in the repository path field above."),
            button(ctx,"Clone repository",Fn("clone_repo"),(),true),
            small("Full-context diffs  /  Editable results  /  Three-way conflict resolution")
        ]).with_style(style().flex_grow().justify_center().padding(px(90)).gap(px(20)))
    } else { row([sidebar(ctx,s),content(ctx)]).with_style(style().flex_grow().min_height(px(0)).min_width(px(0))) };
    column([header,pathbar,confirmation(ctx),body,
        text(ctx.get_state("message")).with_key("status").with_style(style().padding_x(px(18)).padding_y(px(10)).min_height(px(36)).max_height(px(100)).overflow_y_scroll().background(theme_color("surface_raised")).text_color(theme_color("text_muted")))
    ]).with_style(style().width(relative(1)).height(relative(1)).min_width(px(0)).min_height(px(0)).background(theme_color("surface")).text_color(theme_color("text_primary")))
}
