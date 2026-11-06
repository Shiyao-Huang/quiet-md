use fdl_core::docxml;
use fdl_core::markdown;
use fdl_core::model::{Align, Block, BlockKind, Cover, Document, InlineKind, RichText};
use fdl_core::paste::{markdown_to_blocks, Off};
use fdl_core::store::{Entry, Meta, PageWidth, Store, TrashEntry};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const NAME: &str = "quiet";

#[derive(Clone, Debug)]
struct Options {
    library: Option<PathBuf>,
    format: OutputFormat,
    jq: Option<String>,
    dry_run: bool,
    yes: bool,
    help: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Pretty,
    Table,
    Ndjson,
    Csv,
}

impl OutputFormat {
    fn parse(s: &str) -> Result<Self, CliError> {
        match s {
            "json" => Ok(Self::Json),
            "pretty" => Ok(Self::Pretty),
            "table" => Ok(Self::Table),
            "ndjson" => Ok(Self::Ndjson),
            "csv" => Ok(Self::Csv),
            _ => Err(CliError::usage(format!("不支持的输出格式：{s}"), "可选值：json、pretty、table、ndjson、csv")),
        }
    }
}

#[derive(Debug)]
struct CliError {
    kind: &'static str,
    message: String,
    hint: Option<String>,
    code: i32,
}

impl CliError {
    fn usage(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { kind: "usage", message: message.into(), hint: Some(hint.into()), code: 2 }
    }

    fn validation(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { kind: "validation", message: message.into(), hint: Some(hint.into()), code: 2 }
    }

    fn io(message: impl Into<String>) -> Self {
        Self { kind: "io", message: message.into(), hint: None, code: 1 }
    }

    fn conflict(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { kind: "conflict", message: message.into(), hint: Some(hint.into()), code: 1 }
    }

    fn unsupported(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self { kind: "unsupported", message: message.into(), hint: Some(hint.into()), code: 1 }
    }
}

impl From<io::Error> for CliError {
    fn from(e: io::Error) -> Self { Self::io(e.to_string()) }
}

impl From<anyhow::Error> for CliError {
    fn from(e: anyhow::Error) -> Self { Self::io(format!("{e:#}")) }
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    match run_inner() {
        Ok(()) => 0,
        Err(e) => {
            emit_error(&e);
            e.code
        }
    }
}

fn run_inner() -> Result<(), CliError> {
    let raw: Vec<String> = env::args().skip(1).collect();
    let (options, args) = parse_global(raw)?;
    if options.help || args.is_empty() {
        print_help(&args);
        return Ok(());
    }
    let data = dispatch(&args, &options)?;
    emit_success(data, &options);
    Ok(())
}

fn parse_global(raw: Vec<String>) -> Result<(Options, Vec<String>), CliError> {
    let mut options = Options { library: None, format: OutputFormat::Json, jq: None, dry_run: false, yes: false, help: false };
    let mut args = vec![];
    let mut i = 0;
    while i < raw.len() {
        let a = &raw[i];
        if a == "--help" || a == "-h" {
            options.help = true;
            i += 1;
            continue;
        }
        if a == "--version" || a == "-v" {
            println!("{NAME} {VERSION}");
            std::process::exit(0);
        }
        if let Some(v) = a.strip_prefix("--library=") {
            options.library = Some(PathBuf::from(v));
        } else if a == "--library" {
            options.library = Some(PathBuf::from(next_value(&raw, &mut i, "--library")?));
        } else if let Some(v) = a.strip_prefix("--format=") {
            options.format = OutputFormat::parse(v)?;
        } else if a == "--format" {
            options.format = OutputFormat::parse(&next_value(&raw, &mut i, "--format")?)?;
        } else if let Some(v) = a.strip_prefix("--jq=") {
            options.jq = Some(v.to_string());
        } else if a == "--jq" {
            options.jq = Some(next_value(&raw, &mut i, "--jq")?);
        } else if a == "--dry-run" {
            options.dry_run = true;
        } else if a == "--yes" {
            options.yes = true;
        } else {
            args.push(a.clone());
        }
        i += 1;
    }
    Ok((options, args))
}

fn next_value(args: &[String], i: &mut usize, name: &str) -> Result<String, CliError> {
    *i += 1;
    args.get(*i).cloned().ok_or_else(|| CliError::usage(format!("{name} 需要参数"), "运行 `quiet --help` 查看全局参数"))
}

fn dispatch(args: &[String], options: &Options) -> Result<Value, CliError> {
    match args.first().map(String::as_str) {
        Some("docs") | Some("doc") => docs_command(&args[1..], options),
        Some("library") => library_command(&args[1..], options),
        Some("trash") => trash_command(&args[1..], options),
        Some("assets") => assets_command(&args[1..], options),
        Some("emoji") => emoji_command(&args[1..]),
        Some("schema") => schema_command(&args[1..]),
        Some("app") => app_command(&args[1..], options),
        Some("help") => {
            print_help(&args[1..]);
            Ok(Value::Null)
        }
        Some(command) => Err(CliError::usage(format!("未知命令：{command}"), "运行 `quiet --help` 查看可用业务域")),
        None => unreachable!(),
    }
}

fn docs_command(args: &[String], options: &Options) -> Result<Value, CliError> {
    let command = args.first().map(String::as_str).unwrap_or("+list");
    if command == "help" {
        print_help(&["docs".into()]);
        return Ok(Value::Null);
    }
    let mut store = open_store(options)?;
    match command {
        "+list" | "list" => {
            let query = flag(args, "--query");
            let parent = flag(args, "--parent");
            let pinned = bool_flag(args, "--pinned");
            let starred = bool_flag(args, "--starred");
            let mut items: Vec<Entry> = store.list().into_iter().filter(|e| {
                query.as_ref().is_none_or(|q| e.title.to_lowercase().contains(&q.to_lowercase()) || e.body.to_lowercase().contains(&q.to_lowercase()))
                    && parent.as_ref().is_none_or(|p| if p == "root" { e.meta.parent.is_none() } else { e.meta.parent.as_deref() == Some(p) })
                    && pinned.is_none_or(|v| e.meta.pinned == v)
                    && starred.is_none_or(|v| (e.meta.starred > 0) == v)
            }).collect();
            items.sort_by(|a, b| b.meta.modified.cmp(&a.meta.modified).then_with(|| a.id.cmp(&b.id)));
            Ok(json!({ "items": items.iter().map(entry_value).collect::<Vec<_>>() }))
        }
        "+get" | "get" => {
            let id = required_positional(args, 1, "文档 ID")?;
            let doc = store.load(&id)?;
            let entry = store.list().into_iter().find(|e| e.id == id).ok_or_else(|| CliError::validation("文档不存在", "使用 `quiet docs +list` 查看文档 ID"))?;
            if has_flag(args, "--metadata-only") {
                return Ok(json!({ "document": entry_value(&entry) }));
            }
            Ok(json!({ "document": document_value(&id, &entry.meta, &doc, &store, true)? }))
        }
        "+create" | "create" => create_document(args, options, &mut store),
        "+update" | "update" => update_document(args, options, &mut store),
        "+append" | "append" => append_document(args, options, &mut store),
        "+rename" | "rename" => rename_document(args, options, &mut store),
        "+duplicate" | "duplicate" => duplicate_document(args, options, &mut store),
        "+move" | "move" => move_document(args, options, &mut store),
        "+delete" | "delete" => delete_documents(args, options, &mut store),
        "+search" | "search" => search_documents(args, &mut store),
        "+import" | "import" => import_documents(args, options, &mut store),
        "+export" | "export" => export_document(args, options, &mut store),
        "+metadata" | "metadata" => set_metadata(args, options, &mut store),
        "+pin" | "pin" => toggle_meta(args, options, &mut store, "pinned"),
        "+star" | "star" => toggle_meta(args, options, &mut store, "starred"),
        "+width" | "width" => set_width(args, options, &mut store),
        "+reading" | "reading" => toggle_meta(args, options, &mut store, "reading"),
        "+catalogue" | "catalogue" => toggle_meta(args, options, &mut store, "catalogue"),
        "+info" | "info" => set_info(args, options, &mut store),
        "+refs" | "refs" => set_refs(args, options, &mut store),
        "+links" | "links" => links(args, &mut store),
        "+stats" | "stats" => stats(args, &mut store),
        "+print" | "print" => print_document(args, options, &mut store),
        other => Err(CliError::usage(format!("未知 docs 命令：{other}"), "运行 `quiet docs --help` 查看文档命令")),
    }
}

fn open_store(options: &Options) -> Result<Store, CliError> {
    match &options.library {
        Some(root) => Ok(Store::open_at(root.clone())?),
        None => Ok(Store::open()?),
    }
}

fn entry_value(e: &Entry) -> Value {
    json!({
        "id": e.id,
        "title": fdl_core::model::display_title(&e.title),
        "icon": e.icon,
        "created": e.meta.created,
        "modified": e.meta.modified,
        "opened": e.meta.opened,
        "pinned": e.meta.pinned,
        "starred": e.meta.starred > 0,
        "starred_at": e.meta.starred,
        "parent": e.meta.parent,
        "width": page_width_name(e.meta.width),
        "reading": e.meta.reading,
        "catalogue_closed": e.meta.catalogue_closed,
        "refs_hidden": e.meta.refs_hidden,
        "snippet": snippet(&e.body),
    })
}

fn document_value(id: &str, meta: &Meta, doc: &Document, store: &Store, include_content: bool) -> Result<Value, CliError> {
    let mut value = Map::new();
    value.insert("id".into(), Value::String(id.into()));
    value.insert("title".into(), Value::String(fdl_core::model::display_title(&doc.title.text)));
    value.insert("icon".into(), doc.icon.clone().map_or(Value::Null, Value::String));
    value.insert("cover".into(), doc.cover.as_ref().map_or(Value::Null, |c| json!({ "src": c.src, "x": c.x, "y": c.y })));
    value.insert("created".into(), json!(meta.created));
    value.insert("modified".into(), json!(meta.modified));
    value.insert("opened".into(), json!(meta.opened));
    value.insert("pinned".into(), json!(meta.pinned));
    value.insert("starred".into(), json!(meta.starred > 0));
    value.insert("parent".into(), meta.parent.clone().map_or(Value::Null, Value::String));
    value.insert("width".into(), Value::String(page_width_name(meta.width).into()));
    value.insert("reading".into(), json!(meta.reading));
    value.insert("catalogue_closed".into(), json!(meta.catalogue_closed));
    value.insert("info".into(), serde_json::to_value(meta.info).unwrap_or(Value::Null));
    value.insert("refs_hidden".into(), json!(meta.refs_hidden));
    if include_content {
        let subs = child_titles(store, id);
        let export = markdown::export(doc, &markdown::Options { sub_pages: &subs, assets: None });
        value.insert("plain_text".into(), Value::String(doc.plain_text()));
        value.insert("markdown".into(), Value::String(export.text));
        value.insert("xml".into(), Value::String(docxml::serialize(doc)));
    }
    Ok(Value::Object(value))
}

fn create_document(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let title = flag(args, "--title");
    let input = input_document(args, None, title.as_deref())?;
    let parent = flag(args, "--parent");
    if let Some(p) = parent.as_deref().filter(|p| *p != "root") {
        ensure_document(store, p)?;
    }
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "create", "document": preview_document(&input) }));
    }
    let id = store.create_from(&input)?;
    if let Some(parent) = parent.as_deref().filter(|p| *p != "root") {
        store.set_parent(&id, Some(parent))?;
    }
    let entry = store.list().into_iter().find(|e| e.id == id).ok_or_else(|| CliError::io("创建成功但无法读取新文档"))?;
    Ok(json!({ "document": entry_value(&entry) }))
}

fn update_document(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let old = store.load(&id)?;
    let title = flag(args, "--title");
    let mut doc = input_document(args, Some(&old), title.as_deref())?;
    if title.is_none() && input_kind(args).as_deref() == Some("markdown") && doc.title.is_empty() {
        doc.title = old.title.clone();
    }
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "update", "id": id, "document": preview_document(&doc) }));
    }
    store.save(&id, &doc)?;
    let entry = store.list().into_iter().find(|e| e.id == id).ok_or_else(|| CliError::io("更新成功但无法读取文档"))?;
    Ok(json!({ "document": entry_value(&entry) }))
}

fn append_document(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let mut doc = store.load(&id)?;
    let addition = input_document(args, None, None)?;
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "append", "id": id, "blocks": addition.blocks.len() }));
    }
    if doc.is_blank() {
        doc.blocks.clear();
    }
    doc.blocks.extend(addition.blocks);
    store.save(&id, &doc)?;
    Ok(json!({ "id": id, "appended_blocks": doc.blocks.len() }))
}

fn rename_document(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let title = flag(args, "--title").ok_or_else(|| CliError::usage("缺少 --title", "示例：quiet docs +rename ID --title 新标题"))?;
    ensure_document(store, &id)?;
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "rename", "id": id, "title": title }));
    }
    store.rename(&id, &title)?;
    Ok(json!({ "id": id, "title": title }))
}

fn duplicate_document(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let source = store.load(&id)?;
    let source_entry = store.list().into_iter().find(|e| e.id == id).ok_or_else(|| CliError::validation("文档不存在", "使用 `quiet docs +list` 查看文档 ID"))?;
    let title = flag(args, "--title").unwrap_or_else(|| format!("{} 副本", fdl_core::model::display_title(&source.title.text)));
    let parent = flag(args, "--parent");
    if let Some(p) = parent.as_deref().filter(|p| *p != "root") { ensure_document(store, p)?; }
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "duplicate", "source": id, "title": title, "parent": parent }));
    }
    let mut doc = source;
    doc.title = RichText::plain(title);
    let new_id = store.create_from(&doc)?;
    store.set_width(&new_id, source_entry.meta.width)?;
    if let Some(parent) = parent.as_deref().filter(|p| *p != "root") { store.set_parent(&new_id, Some(parent))?; }
    Ok(json!({ "source": id, "id": new_id, "title": doc.title.text }))
}

fn move_document(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let parent = flag(args, "--parent").ok_or_else(|| CliError::usage("缺少 --parent", "使用 `--parent root` 移到根目录"))?;
    let entries = store.list();
    ensure_entry(&entries, &id)?;
    if parent != "root" {
        ensure_entry(&entries, &parent)?;
        if id == parent || is_descendant(&entries, &parent, &id) {
            return Err(CliError::validation("不能把文档移动到自身或子文档下", "请选择当前文档以外的父文档"));
        }
    }
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "move", "id": id, "parent": parent }));
    }
    let meta = store.set_parent(&id, (parent != "root").then_some(parent.as_str()))?;
    Ok(json!({ "id": id, "parent": meta.parent, "order": meta.order }))
}

fn delete_documents(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    require_yes(options, "delete")?;
    let ids = positionals(args).into_iter().skip(1).collect::<Vec<_>>();
    if ids.is_empty() { return Err(CliError::usage("至少需要一个文档 ID", "示例：quiet docs +delete ID --yes")); }
    let entries = store.list();
    let groups = deletion_groups(&entries, &ids)?;
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "delete", "groups": groups.iter().map(|g| json!({"root": g.root, "ids": g.ids})).collect::<Vec<_>>() }));
    }
    let mut deleted = vec![];
    for group in &groups {
        for id in &group.ids {
            let root = (id != &group.root).then_some(group.root.as_str());
            store.delete(id, root)?;
            deleted.push(id.clone());
        }
    }
    Ok(json!({ "deleted": deleted, "groups": groups.len() }))
}

struct DeletionGroup { root: String, ids: Vec<String> }

fn deletion_groups(entries: &[Entry], ids: &[String]) -> Result<Vec<DeletionGroup>, CliError> {
    for id in ids { ensure_entry(entries, id)?; }
    let selected: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let roots: Vec<String> = ids.iter().filter(|id| !ancestor_ids(entries, id).iter().any(|a| selected.contains(a.as_str()))).cloned().collect();
    let mut groups = vec![];
    for root in roots {
        let mut descendants: Vec<String> = entries.iter().filter(|e| e.id == root || ancestor_ids(entries, &e.id).iter().any(|a| a == &root)).map(|e| e.id.clone()).collect();
        descendants.sort_by_key(|id| ancestor_ids(entries, id).len());
        groups.push(DeletionGroup { root, ids: descendants });
    }
    Ok(groups)
}

fn search_documents(args: &[String], store: &mut Store) -> Result<Value, CliError> {
    let query = positionals(args).into_iter().skip(1).collect::<Vec<_>>().join(" ");
    if query.trim().is_empty() { return Err(CliError::usage("缺少搜索词", "示例：quiet docs +search 研究方法")); }
    let q = query.to_lowercase();
    let mut items = vec![];
    for e in store.list() {
        if e.title.to_lowercase().contains(&q) || e.body.to_lowercase().contains(&q) {
            let mut v = entry_value(&e);
            if let Some(obj) = v.as_object_mut() { obj.insert("matches".into(), json!(match_count(&e.body, &q))); }
            items.push(v);
        }
    }
    Ok(json!({ "query": query, "items": items }))
}

fn import_documents(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let paths = positionals(args).into_iter().skip(1).collect::<Vec<_>>();
    if paths.is_empty() { return Err(CliError::usage("缺少导入文件", "支持 .md、.markdown、.txt 和 .xml")); }
    let mut docs = vec![];
    for raw in &paths {
        let path = Path::new(raw);
        let doc = document_from_path(path)?;
        docs.push((raw.clone(), doc));
    }
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "import", "files": docs.iter().map(|(p,d)| json!({"file":p,"document":preview_document(d)})).collect::<Vec<_>>() }));
    }
    let mut imported = vec![];
    for (path, doc) in docs {
        let id = store.create_from(&doc)?;
        imported.push(json!({ "id": id, "file": path, "title": fdl_core::model::display_title(&doc.title.text) }));
    }
    Ok(json!({ "items": imported }))
}

fn export_document(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let target = flag(args, "--as").unwrap_or_else(|| "markdown".into()).to_lowercase();
    let output = flag(args, "--output").map(PathBuf::from);
    let all = has_flag(args, "--all");
    let entries = store.list();
    let entry = entries.iter().find(|e| e.id == id).ok_or_else(|| CliError::validation("文档不存在", "使用 `quiet docs +list` 查看文档 ID"))?;
    let mut doc = store.load(&id)?;
    if all { absolutize_assets(&mut doc, &library_root(options)); }
    let subs = child_titles_from_entries(&entries, &id);
    let text = match target.as_str() {
        "md" | "markdown" => markdown::export(&doc, &markdown::Options { sub_pages: &subs, assets: all.then_some("assets") }).text,
        "xml" | "docxxml" => docxml::serialize(&doc),
        "html" => render_html(&doc, &subs, &library_root(options)),
        "pdf" => {
            let path = output.clone().ok_or_else(|| CliError::usage("PDF 导出需要 --output", "示例：quiet docs +export ID --as pdf --output note.pdf"))?;
            if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "export", "id": id, "as": target, "output": path })); }
            let html = render_html(&doc, &subs, &library_root(options));
            write_pdf(&html, &path)?;
            return Ok(json!({ "id": id, "format": target, "output": path, "bytes": fs::metadata(path).map(|m|m.len()).unwrap_or(0) }));
        }
        other => return Err(CliError::validation(format!("不支持导出格式：{other}"), "可选值：markdown、xml、html、pdf")),
    };
    if options.dry_run {
        return Ok(json!({ "dry_run": true, "operation": "export", "id": id, "as": target, "output": output, "bytes": text.len() }));
    }
    match output {
        Some(path) => {
            write_export_files(&path, &text, &doc, all, &library_root(options))?;
            Ok(json!({ "id": id, "format": target, "output": path, "bytes": text.len(), "title": fdl_core::model::display_title(&entry.title) }))
        }
        None => Ok(json!({ "id": id, "format": target, "content": text, "title": fdl_core::model::display_title(&entry.title) })),
    }
}

fn write_export_files(path: &Path, text: &str, doc: &Document, all: bool, root: &Path) -> Result<(), CliError> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    fs::write(path, text)?;
    if all {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("doc");
        let asset_dir = dir.join(format!("{stem}.assets"));
        for src in doc.asset_sources() {
            if let Some(source) = asset_path(root, &src) {
                let name = source.file_name().and_then(|s| s.to_str()).unwrap_or("asset").to_string();
                let _ = fs::create_dir_all(&asset_dir);
                let _ = fs::copy(source, asset_dir.join(name));
            }
        }
    }
    Ok(())
}

fn toggle_meta(args: &[String], options: &Options, store: &mut Store, field: &str) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let value = flag(args, "--set").or_else(|| flag(args, "--value")).and_then(|v| parse_bool(&v));
    let value = value.unwrap_or(!has_flag(args, "--off"));
    ensure_document(store, &id)?;
    if options.dry_run { return Ok(json!({ "dry_run": true, "operation": field, "id": id, "value": value })); }
    match field {
        "pinned" => store.set_pinned(&id, value)?,
        "starred" => { store.set_starred(&id, value)?; }
        "reading" => { store.update_meta(&id, |m| m.reading = value)?; }
        "catalogue" => { store.set_catalogue(&id, value)?; }
        _ => unreachable!(),
    }
    Ok(json!({ "id": id, field: value }))
}

fn set_width(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let width = flag(args, "--width").or_else(|| positionals(args).get(2).cloned()).ok_or_else(|| CliError::usage("缺少页宽", "可选值：standard、wide、full"))?;
    let width = parse_width(&width)?;
    ensure_document(store, &id)?;
    if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "width", "id": id, "width": page_width_name(width) })); }
    store.set_width(&id, width)?;
    Ok(json!({ "id": id, "width": page_width_name(width) }))
}

fn set_metadata(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let mut doc = store.load(&id)?;
    let mut changed = false;
    if let Some(icon) = flag(args, "--icon") {
        doc.icon = Some(icon);
        changed = true;
    }
    if has_flag(args, "--clear-icon") {
        doc.icon = None;
        changed = true;
    }
    if let Some(src) = flag(args, "--cover") {
        let x = flag(args, "--cover-x").map_or(Ok(0.), |v| v.parse::<f32>().map_err(|_| CliError::validation("--cover-x 必须是数字", "")))?;
        let y = flag(args, "--cover-y").map_or(Ok(0.), |v| v.parse::<f32>().map_err(|_| CliError::validation("--cover-y 必须是数字", "")))?;
        doc.cover = Some(Cover { src, x, y });
        changed = true;
    }
    if has_flag(args, "--clear-cover") {
        doc.cover = None;
        changed = true;
    }
    if !changed { return Err(CliError::usage("没有元数据修改", "使用 --icon、--clear-icon、--cover 或 --clear-cover")); }
    if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "metadata", "id": id, "icon": doc.icon, "cover": doc.cover.as_ref().map(|c| json!({"src": c.src, "x": c.x, "y": c.y})) })); }
    store.save(&id, &doc)?;
    Ok(json!({ "id": id, "icon": doc.icon, "cover": doc.cover.as_ref().map(|c| json!({"src": c.src, "x": c.x, "y": c.y})) }))
}

fn set_info(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let old = store.list().into_iter().find(|e| e.id == id).ok_or_else(|| CliError::validation("文档不存在", "使用 `quiet docs +list` 查看文档 ID"))?;
    let mut info = old.meta.info;
    if let Some(v) = flag(args, "--hidden") { info.hidden = parse_bool(&v).ok_or_else(|| CliError::validation("--hidden 必须是 true 或 false", ""))?; }
    if let Some(v) = flag(args, "--created") { info.created = parse_bool(&v).ok_or_else(|| CliError::validation("--created 必须是 true 或 false", ""))?; }
    if let Some(v) = flag(args, "--modified") { info.modified = parse_bool(&v).ok_or_else(|| CliError::validation("--modified 必须是 true 或 false", ""))?; }
    if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "info", "id": id, "info": info })); }
    store.update_meta(&id, |m| m.info = info)?;
    Ok(json!({ "id": id, "info": info }))
}

fn set_refs(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    ensure_document(store, &id)?;
    let show = if has_flag(args, "--show") { true } else if has_flag(args, "--hide") { false } else { !has_flag(args, "--off") };
    let no_ask = bool_flag(args, "--no-ask").unwrap_or(false);
    if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "refs", "id": id, "show": show, "no_ask": no_ask })); }
    store.update_meta(&id, |m| { m.refs_hidden = !show; })?;
    Ok(json!({ "id": id, "show": show, "no_ask": no_ask }))
}

fn links(args: &[String], store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let entries = store.list();
    ensure_entry(&entries, &id)?;
    let mut out = vec![];
    for e in &entries {
        for link in &e.links {
            if link.target == id {
                out.push(json!({ "source": e.id, "source_title": fdl_core::model::display_title(&e.title), "block": link.block, "line": link.line }));
            }
        }
    }
    Ok(json!({ "id": id, "items": out }))
}

fn stats(args: &[String], store: &mut Store) -> Result<Value, CliError> {
    let id = required_positional(args, 1, "文档 ID")?;
    let doc = store.load(&id)?;
    let mut blocks = 0usize;
    let mut tables = 0usize;
    doc.for_each_block(|b| { blocks += 1; if matches!(b.kind, BlockKind::Table(_)) { tables += 1; } });
    let assets = doc.asset_sources().len();
    let text = doc.plain_text();
    Ok(json!({ "id": id, "title": fdl_core::model::display_title(&doc.title.text), "blocks": blocks, "tables": tables, "assets": assets, "characters": text.chars().count(), "words": text.split_whitespace().count(), "bytes": store.size_of(&id, &doc) }))
}

fn print_document(args: &[String], options: &Options, store: &mut Store) -> Result<Value, CliError> {
    require_yes(options, "print")?;
    let id = required_positional(args, 1, "文档 ID")?;
    let doc = store.load(&id)?;
    let entries = store.list();
    let subs = child_titles_from_entries(&entries, &id);
    if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "print", "id": id })); }
    let temp = env::temp_dir().join(format!("quiet-print-{}-{}.pdf", std::process::id(), unix_now()));
    write_pdf(&render_html(&doc, &subs, &library_root(options)), &temp)?;
    let status = Command::new("lp").arg(&temp).status().map_err(|e| CliError::unsupported("当前系统没有 lp 打印命令", e.to_string()))?;
    let _ = fs::remove_file(&temp);
    if !status.success() { return Err(CliError::io("打印命令失败")); }
    Ok(json!({ "id": id, "printed": true }))
}

fn library_command(args: &[String], options: &Options) -> Result<Value, CliError> {
    let command = args.first().map(String::as_str).unwrap_or("status");
    let mut store = open_store(options)?;
    match command {
        "path" => Ok(json!({ "path": library_root(options) })),
        "status" => {
            let docs = store.list();
            let trash = store.list_trash();
            Ok(json!({ "path": library_root(options), "format": 1, "documents": docs.len(), "trash": trash.len(), "warnings": store.warnings }))
        }
        "list" => {
            let docs = store.list();
            Ok(json!({ "items": docs.iter().map(entry_value).collect::<Vec<_>>() }))
        }
        "tree" => {
            let docs = store.list();
            Ok(json!({ "items": tree_value(&docs) }))
        }
        "doctor" | "validate" => doctor(&mut store, options),
        "settings" => settings_command(args, options, &store),
        other => Err(CliError::usage(format!("未知 library 命令：{other}"), "可选值：path、status、list、tree、doctor、validate、settings")),
    }
}

fn doctor(store: &mut Store, options: &Options) -> Result<Value, CliError> {
    let root = library_root(options);
    let docs = store.list();
    let mut errors = vec![];
    let mut warnings = store.warnings.clone();
    let ids: HashSet<&str> = docs.iter().map(|e| e.id.as_str()).collect();
    for e in &docs {
        if let Some(parent) = &e.meta.parent && !ids.contains(parent.as_str()) { errors.push(json!({ "id": e.id, "kind": "missing_parent", "parent": parent })); }
        match store.load(&e.id) {
            Ok(doc) => {
                for src in doc.asset_sources() {
                    if let Some(path) = asset_path(&root, &src) && !path.exists() && !src.starts_with("http") {
                        warnings.push(format!("{} 引用了不存在的素材：{}", e.id, src));
                    }
                }
                for link in doc.doc_links() {
                    if !ids.contains(link.target.as_str()) { warnings.push(format!("{} 引用了不存在的文档：{}", e.id, link.target)); }
                }
            }
            Err(err) => errors.push(json!({ "id": e.id, "kind": "invalid_xml", "message": err.to_string() })),
        }
    }
    let healthy = errors.is_empty();
    Ok(json!({ "healthy": healthy, "path": root, "documents": docs.len(), "errors": errors, "warnings": warnings }))
}

fn settings_command(args: &[String], options: &Options, store: &Store) -> Result<Value, CliError> {
    let sub = args.get(1).map(String::as_str).unwrap_or("get");
    if sub == "get" { return Ok(json!({ "settings": store.settings() })); }
    if sub != "set" { return Err(CliError::usage(format!("未知 settings 命令：{sub}"), "可选值：get、set")); }
    let mut settings = store.settings();
    if let Some(v) = flag(args, "--home-list") { settings.home_list = parse_bool(&v).ok_or_else(|| CliError::validation("--home-list 必须是 true 或 false", ""))?; }
    if let Some(v) = flag(args, "--home-sort") { settings.home_sort = Some(v); }
    if let Some(v) = flag(args, "--sidebar-width") { settings.sidebar_width = Some(v.parse().map_err(|_| CliError::validation("--sidebar-width 必须是数字", ""))?); }
    if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "settings.set", "settings": settings })); }
    let writable = open_store(options)?;
    writable.save_settings(&settings)?;
    Ok(json!({ "settings": settings }))
}

fn trash_command(args: &[String], options: &Options) -> Result<Value, CliError> {
    let command = args.first().map(String::as_str).unwrap_or("list");
    let mut store = open_store(options)?;
    match command {
        "list" => Ok(json!({ "items": store.list_trash().iter().map(trash_value).collect::<Vec<_>>() })),
        "restore" => {
            require_yes(options, "restore")?;
            let id = required_positional(args, 1, "回收站文档 ID")?;
            if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "restore", "id": id })); }
            let moved = store.restore(&id)?;
            Ok(json!({ "id": id, "restored": true, "parent_missing": moved }))
        }
        "purge" => {
            require_yes(options, "purge")?;
            let id = required_positional(args, 1, "回收站文档 ID")?;
            if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "purge", "id": id })); }
            store.purge(&id)?;
            Ok(json!({ "id": id, "purged": true }))
        }
        "empty" => {
            require_yes(options, "empty")?;
            let ids: Vec<String> = store.list_trash().into_iter().map(|e| e.id).collect();
            if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "empty", "ids": ids })); }
            for id in &ids { store.purge(id)?; }
            Ok(json!({ "purged": ids }))
        }
        other => Err(CliError::usage(format!("未知 trash 命令：{other}"), "可选值：list、restore、purge、empty")),
    }
}

fn assets_command(args: &[String], options: &Options) -> Result<Value, CliError> {
    let id = positionals(args).get(1).cloned();
    let mut store = open_store(options)?;
    let docs = store.list();
    let selected: Vec<&Entry> = docs.iter().filter(|e| id.as_ref().is_none_or(|wanted| &e.id == wanted)).collect();
    if selected.is_empty() && id.is_some() { return Err(CliError::validation("文档不存在", "使用 `quiet docs +list` 查看文档 ID")); }
    let root = library_root(options);
    let mut items = vec![];
    for e in selected {
        let doc = store.load(&e.id)?;
        for src in doc.asset_sources() {
            let path = asset_path(&root, &src);
            items.push(json!({ "document": e.id, "source": src, "path": path, "exists": path.as_ref().is_some_and(|p| p.exists()) }));
        }
    }
    Ok(json!({ "items": items }))
}

fn emoji_command(args: &[String]) -> Result<Value, CliError> {
    let query = positionals(args).into_iter().skip(1).collect::<Vec<_>>().join(" ");
    if query.trim().is_empty() { return Err(CliError::usage("缺少 emoji 搜索词", "示例：quiet emoji search 猫")); }
    let mut items = vec![];
    for line in fdl_core::emoji::TSV.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 6 { continue; }
        let hay = fields[1..].join(" ").to_lowercase();
        if query.to_lowercase().split_whitespace().all(|q| hay.contains(q)) {
            items.push(json!({ "id": fields[1], "char": fields[2], "name_zh": fields.get(3), "name_en": fields.get(4), "keywords": fields.get(5) }));
            if items.len() >= 90 { break; }
        }
    }
    Ok(json!({ "query": query, "items": items }))
}

fn schema_command(args: &[String]) -> Result<Value, CliError> {
    let schemas = command_schemas();
    if !args.is_empty() {
        let name = args.join(" ");
        return schemas.into_iter().find(|s| s["name"] == name).map_or_else(|| Err(CliError::validation("找不到命令 schema", "运行 `quiet schema` 查看完整列表")), Ok);
    }
    Ok(json!({ "items": schemas }))
}

fn command_schemas() -> Vec<Value> {
    [
        ("docs +list", "read", "列出本地文档", "quiet docs +list [--query TEXT] [--parent ID|root]"),
        ("docs +get", "read", "读取文档和内容", "quiet docs +get ID"),
        ("docs +create", "write", "创建文档", "quiet docs +create --title TITLE [--file PATH|--content TEXT]"),
        ("docs +update", "write", "替换文档内容", "quiet docs +update ID --file PATH --content-format markdown|xml"),
        ("docs +append", "write", "追加 Markdown/XML block", "quiet docs +append ID --file PATH"),
        ("docs +metadata", "write", "设置图标和封面", "quiet docs +metadata ID --icon EMOJI --cover PATH"),
        ("docs +delete", "high-risk-write", "把文档树移入回收站", "quiet docs +delete ID --yes"),
        ("docs +import", "write", "导入 Markdown/TXT/DocxXML", "quiet docs +import FILE..."),
        ("docs +export", "read", "导出 Markdown/XML/HTML/PDF", "quiet docs +export ID --as FORMAT"),
        ("docs +search", "read", "搜索标题和正文", "quiet docs +search QUERY"),
        ("trash restore", "high-risk-write", "恢复回收站文档", "quiet trash restore ID --yes"),
        ("trash purge", "high-risk-write", "永久删除回收站文档", "quiet trash purge ID --yes"),
        ("library doctor", "read", "校验索引、XML、父子关系和素材", "quiet library doctor"),
        ("emoji search", "read", "搜索内置 emoji 数据", "quiet emoji search QUERY"),
    ].into_iter().map(|(name, risk, description, usage)| json!({ "name": name, "risk": risk, "description": description, "usage": usage })).collect()
}

fn app_command(args: &[String], options: &Options) -> Result<Value, CliError> {
    let command = args.first().map(String::as_str).unwrap_or("open");
    if command != "open" { return Err(CliError::usage(format!("未知 app 命令：{command}"), "可选值：open")); }
    if options.dry_run { return Ok(json!({ "dry_run": true, "operation": "app.open" })); }
    #[cfg(target_os = "macos")]
    let status = Command::new("open").args(["-a", "Quiet.md"]).status()?;
    #[cfg(target_os = "windows")]
    let status = Command::new("cmd").args(["/C", "start", "", "quiet.md"]).status()?;
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let status = Command::new("quiet_md").status()?;
    if !status.success() { return Err(CliError::io("无法启动 Quiet.md GUI")); }
    Ok(json!({ "opened": true }))
}

fn input_document(args: &[String], base: Option<&Document>, title: Option<&str>) -> Result<Document, CliError> {
    let source = if has_flag(args, "--stdin") {
        let mut text = String::new();
        io::stdin().read_to_string(&mut text)?;
        Some((text, input_kind(args).unwrap_or_else(|| "markdown".into())))
    } else if let Some(text) = flag(args, "--content") {
        Some((text, input_kind(args).unwrap_or_else(|| "markdown".into())))
    } else if let Some(path) = flag(args, "--file") {
        let path_ref = Path::new(&path);
        let text = fs::read_to_string(path_ref)?;
        let kind = input_kind(args).unwrap_or_else(|| format_from_extension(path_ref));
        Some((text, kind))
    } else {
        None
    };
    let mut doc = match source {
        Some((text, kind)) if kind == "xml" || kind == "docxxml" => docxml::parse(&text)?,
        Some((text, _)) => markdown_document(&text, None),
        None => base.cloned().unwrap_or_default(),
    };
    if let Some(title) = title { doc.title = RichText::plain(title); }
    Ok(doc)
}

fn document_from_path(path: &Path) -> Result<Document, CliError> {
    let ext = format_from_extension(path);
    if ext == "xml" || ext == "docxxml" { return Ok(docxml::parse(&fs::read_to_string(path)?)?); }
    let text = fs::read_to_string(path)?;
    Ok(markdown_document(&text, path.file_stem().and_then(|s| s.to_str())))
}

fn markdown_document(text: &str, stem: Option<&str>) -> Document {
    let mut blocks = markdown_to_blocks(text.trim_start_matches('\u{feff}'), Off { file: true, ..Default::default() });
    let title = stem.unwrap_or("").trim();
    if blocks.first().is_some_and(|b| matches!(b.kind, BlockKind::Heading { level: 1, .. }) && !title.is_empty() && b.text.text.trim() == title) { blocks.remove(0); }
    if blocks.is_empty() { blocks.push(Block::text("")); }
    Document { title: RichText::plain(title), icon: None, cover: None, blocks }
}

fn input_kind(args: &[String]) -> Option<String> {
    flag(args, "--content-format").map(|s| s.to_lowercase())
}

fn format_from_extension(path: &Path) -> String {
    match path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
        "xml" => "xml".into(),
        _ => "markdown".into(),
    }
}

fn preview_document(doc: &Document) -> Value {
    json!({ "title": fdl_core::model::display_title(&doc.title.text), "blocks": doc.blocks.len(), "characters": doc.plain_text().chars().count() })
}

fn ensure_document(store: &mut Store, id: &str) -> Result<(), CliError> {
    store.load(id).map(|_| ()).map_err(|_| CliError::validation(format!("文档不存在：{id}"), "使用 `quiet docs +list` 查看文档 ID"))
}

fn ensure_entry(entries: &[Entry], id: &str) -> Result<(), CliError> {
    entries.iter().any(|e| e.id == id).then_some(()).ok_or_else(|| CliError::validation(format!("文档不存在：{id}"), "使用 `quiet docs +list` 查看文档 ID"))
}

fn page_width_name(width: PageWidth) -> &'static str {
    match width { PageWidth::Standard => "standard", PageWidth::Wide => "wide", PageWidth::Full => "full" }
}

fn parse_width(s: &str) -> Result<PageWidth, CliError> {
    match s.to_ascii_lowercase().as_str() {
        "standard" | "default" => Ok(PageWidth::Standard),
        "wide" => Ok(PageWidth::Wide),
        "full" => Ok(PageWidth::Full),
        _ => Err(CliError::validation(format!("不支持的页宽：{s}"), "可选值：standard、wide、full")),
    }
}

fn require_yes(options: &Options, operation: &str) -> Result<(), CliError> {
    if options.yes || options.dry_run { return Ok(()); }
    Err(CliError::conflict(format!("{operation} 是 high-risk-write，必须显式传入 --yes"), "先使用 --dry-run 预览，再使用 --yes 执行"))
}

fn positionals(args: &[String]) -> Vec<String> {
    let value_flags: HashSet<&str> = ["--query", "--parent", "--title", "--file", "--content", "--content-format", "--as", "--output", "--set", "--value", "--width", "--icon", "--cover", "--cover-x", "--cover-y", "--hidden", "--created", "--modified", "--home-list", "--home-sort", "--sidebar-width", "--no-ask"].into_iter().collect();
    let mut out = vec![];
    let mut skip = false;
    for a in args {
        if skip { skip = false; continue; }
        if a == "--stdin" || a == "--dry-run" || a == "--yes" || a == "--all" || a == "--metadata-only" || a == "--show" || a == "--hide" || a == "--off" { continue; }
        if a.starts_with("--") {
            if let Some((name, _)) = a.split_once('=') {
                if value_flags.contains(name) { continue; }
            } else if value_flags.contains(a.as_str()) { skip = true; }
            continue;
        }
        out.push(a.clone());
    }
    out
}

fn required_positional(args: &[String], index: usize, label: &str) -> Result<String, CliError> {
    positionals(args).get(index).cloned().ok_or_else(|| CliError::usage(format!("缺少{label}"), "运行对应命令的 --help 查看示例"))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().enumerate().find_map(|(i, a)| {
        a.strip_prefix(&format!("{name}="))
            .map(str::to_string)
            .or_else(|| (a == name).then(|| args.get(i + 1).cloned()).flatten())
    })
}

fn has_flag(args: &[String], name: &str) -> bool { args.iter().any(|a| a == name || a.starts_with(&format!("{name}="))) }

fn bool_flag(args: &[String], name: &str) -> Option<bool> { flag(args, name).and_then(|v| parse_bool(&v)).or_else(|| has_flag(args, name).then_some(true)) }

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() { "true" | "1" | "yes" | "on" => Some(true), "false" | "0" | "no" | "off" => Some(false), _ => None }
}

fn snippet(s: &str) -> String { s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect() }

fn match_count(text: &str, query: &str) -> usize {
    if query.is_empty() { return 0; }
    text.to_lowercase().match_indices(query).count()
}

fn ancestor_ids(entries: &[Entry], id: &str) -> Vec<String> {
    let by_id: HashMap<&str, &Entry> = entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut out = vec![];
    let mut current = id;
    let mut seen = HashSet::new();
    while let Some(parent) = by_id.get(current).and_then(|e| e.meta.parent.as_deref()) {
        if !seen.insert(parent) { break; }
        out.push(parent.to_string());
        current = parent;
    }
    out
}

fn is_descendant(entries: &[Entry], id: &str, ancestor: &str) -> bool { ancestor_ids(entries, id).iter().any(|a| a == ancestor) }

fn child_titles(store: &Store, id: &str) -> Vec<String> {
    let mut copy = match Store::open_at(store.root().to_path_buf()) { Ok(mut s) => s.list(), Err(_) => vec![] };
    copy.sort_by_key(|e| e.meta.sibling_key());
    copy.into_iter().filter(|e| e.meta.parent.as_deref() == Some(id)).map(|e| fdl_core::model::display_title(&e.title)).collect()
}

fn child_titles_from_entries(entries: &[Entry], id: &str) -> Vec<String> {
    let mut out: Vec<&Entry> = entries.iter().filter(|e| e.meta.parent.as_deref() == Some(id)).collect();
    out.sort_by_key(|e| e.meta.sibling_key());
    out.into_iter().map(|e| fdl_core::model::display_title(&e.title)).collect()
}

fn tree_value(entries: &[Entry]) -> Vec<Value> {
    let mut children: HashMap<Option<String>, Vec<&Entry>> = HashMap::new();
    for e in entries { children.entry(e.meta.parent.clone()).or_default().push(e); }
    for group in children.values_mut() { group.sort_by_key(|e| e.meta.sibling_key()); }
    fn walk(parent: Option<&str>, groups: &HashMap<Option<String>, Vec<&Entry>>) -> Vec<Value> {
        groups.get(&parent.map(str::to_string)).map(|items| items.iter().map(|e| {
            json!({ "id": e.id, "title": fdl_core::model::display_title(&e.title), "children": walk(Some(e.id.as_str()), groups) })
        }).collect()).unwrap_or_default()
    }
    walk(None, &children)
}

fn trash_value(e: &TrashEntry) -> Value { json!({ "id": e.id, "title": fdl_core::model::display_title(&e.title), "icon": e.icon, "deleted": e.deleted, "parent": e.parent }) }

fn library_root(options: &Options) -> PathBuf { options.library.clone().unwrap_or_else(Store::root_dir) }

fn asset_path(root: &Path, src: &str) -> Option<PathBuf> {
    if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("data:") { return None; }
    let path = PathBuf::from(src.strip_prefix("file://").unwrap_or(src));
    if path.is_absolute() { Some(path) } else if src.contains('/') { Some(root.join(path)) } else { None }
}

fn absolutize_assets(doc: &mut Document, root: &Path) {
    fn walk(blocks: &mut [Block], root: &Path) {
        for b in blocks {
            if let BlockKind::Image { src, .. } = &mut b.kind && let Some(path) = asset_path(root, src) { *src = path.to_string_lossy().into_owned(); }
            if let BlockKind::File(f) = &mut b.kind && let Some(path) = asset_path(root, &f.src) { f.src = path.to_string_lossy().into_owned(); }
            walk(&mut b.children, root);
            if let BlockKind::Table(t) = &mut b.kind { for cell in t.rows.iter_mut().flatten() { walk(&mut cell.blocks, root); } }
        }
    }
    walk(&mut doc.blocks, root);
}

fn write_pdf(html: &str, output: &Path) -> Result<(), CliError> {
    if let Some(parent) = output.parent() { fs::create_dir_all(parent)?; }
    let temp = env::temp_dir().join(format!("quiet-html-{}-{}.html", std::process::id(), unix_now()));
    fs::write(&temp, html)?;
    let mut converted = false;
    if command_exists("pandoc") {
        converted = Command::new("pandoc").args(["--from", "html", "--output"]).arg(output).arg(&temp).status().map(|s| s.success()).unwrap_or(false);
    }
    if !converted && command_exists("cupsfilter") {
        converted = Command::new("cupsfilter").args(["-m", "application/pdf", "-i", "text/html", "-o"]).arg(output).arg(&temp).status().map(|s| s.success()).unwrap_or(false);
    }
    let _ = fs::remove_file(&temp);
    if !converted { return Err(CliError::unsupported("无法生成 PDF", "安装 pandoc 或使用支持 HTML 打印的系统后重试")); }
    Ok(())
}

fn command_exists(name: &str) -> bool { Command::new("sh").args(["-c", &format!("command -v {name} >/dev/null 2>&1")]).status().is_ok_and(|s| s.success()) }
fn unix_now() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) }

fn render_html(doc: &Document, sub_pages: &[String], root: &Path) -> String {
    let title = fdl_core::model::display_title(&doc.title.text);
    let mut out = format!("<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\"><title>{}</title><style>body{{font:15px/1.6 -apple-system,BlinkMacSystemFont,sans-serif;max-width:900px;margin:40px auto;padding:0 24px;color:#1f2225}}h1{{border-bottom:1px solid #ddd;padding-bottom:12px}}pre{{background:#f6f7f8;padding:12px;overflow:auto}}blockquote{{border-left:4px solid #aaa;padding-left:16px;color:#555}}table{{border-collapse:collapse}}td,th{{border:1px solid #ddd;padding:6px 9px;vertical-align:top}}img{{max-width:100%}}.callout{{padding:12px;border:1px solid #ddd;background:#fff8e5}}.todo.done{{text-decoration:line-through;color:#777}}</style></head><body><h1>{}</h1>", esc(&title), esc(&title));
    if let Some(cover) = &doc.cover && let Some(uri) = data_uri(root, &cover.src) { out.push_str(&format!("<img src=\"{uri}\" alt=\"cover\">")); }
    render_blocks(&doc.blocks, &mut out, root, sub_pages);
    out.push_str("</body></html>\n");
    out
}

fn render_blocks(blocks: &[Block], out: &mut String, root: &Path, sub_pages: &[String]) {
    for b in blocks { render_block(b, out, root, sub_pages); }
}

fn render_block(b: &Block, out: &mut String, root: &Path, sub_pages: &[String]) {
    let align = match b.align { Align::Left => "", Align::Center => " style=\"text-align:center\"", Align::Right => " style=\"text-align:right\"" };
    match &b.kind {
        BlockKind::Text => { out.push_str(&format!("<p{align}>{}</p>", rich_html(&b.text))); render_blocks(&b.children, out, root, sub_pages); }
        BlockKind::Heading { level, .. } => { let n = (*level).clamp(1, 6); out.push_str(&format!("<h{n}{align}>{}</h{n}>", rich_html(&b.text))); render_blocks(&b.children, out, root, sub_pages); }
        BlockKind::Bullet => { out.push_str("<ul><li>"); out.push_str(&rich_html(&b.text)); if !b.children.is_empty() { out.push_str("<ul>"); render_blocks(&b.children, out, root, sub_pages); out.push_str("</ul>"); } out.push_str("</li></ul>"); }
        BlockKind::Ordered { .. } => { out.push_str("<ol><li>"); out.push_str(&rich_html(&b.text)); render_blocks(&b.children, out, root, sub_pages); out.push_str("</li></ol>"); }
        BlockKind::Todo { done } => { out.push_str(&format!("<p class=\"todo{}\">{} {}</p>", if *done { " done" } else { "" }, if *done { "☑" } else { "☐" }, rich_html(&b.text))); render_blocks(&b.children, out, root, sub_pages); }
        BlockKind::Code { lang, .. } => out.push_str(&format!("<pre><code class=\"language-{}\">{}</code></pre>", esc(lang), esc(&b.text.text))),
        BlockKind::Quote => { out.push_str("<blockquote>"); render_blocks(&b.children, out, root, sub_pages); out.push_str("</blockquote>"); }
        BlockKind::Callout { emoji, .. } => { out.push_str(&format!("<div class=\"callout\">{} ", esc(emoji))); render_blocks(&b.children, out, root, sub_pages); out.push_str("</div>"); }
        BlockKind::Divider => out.push_str("<hr>"),
        BlockKind::Image { src, caption, .. } => {
            out.push_str("<p>");
            match data_uri(root, src) {
                Some(uri) => out.push_str(&format!("<img src=\"{uri}\">")),
                None => out.push_str(&format!("<a href=\"{}\">{}</a>", esc(src), esc(src))),
            }
            if let Some(c) = caption { out.push_str(&format!("<br><small>{}</small>", esc(c))); }
            out.push_str("</p>");
        }
        BlockKind::Table(t) => { out.push_str("<table>"); for (ri, row) in t.rows.iter().enumerate() { out.push_str("<tr>"); for cell in row { out.push_str(if ri < t.header_rows { "<th>" } else { "<td>" }); for child in &cell.blocks { out.push_str(&rich_html(&child.text)); } out.push_str(if ri < t.header_rows { "</th>" } else { "</td>" }); } out.push_str("</tr>"); } out.push_str("</table>"); }
        BlockKind::Grid => { out.push_str("<div style=\"display:flex;gap:16px\">"); for col in &b.children { out.push_str("<div style=\"flex:1\">"); render_blocks(&col.children, out, root, sub_pages); out.push_str("</div>"); } out.push_str("</div>"); }
        BlockKind::Column { .. } => render_blocks(&b.children, out, root, sub_pages),
        BlockKind::SubPageList => { out.push_str("<ul>"); for title in sub_pages { out.push_str(&format!("<li>{}</li>", esc(title))); } out.push_str("</ul>"); }
        BlockKind::Countdown(c) => out.push_str(&format!("<p><strong>倒计时：{} 秒</strong></p>", c.remaining(fdl_core::time::now_ms()))),
        BlockKind::File(f) => { out.push_str(&format!("<p><a href=\"{}\">{}</a></p>", esc(&f.src), esc(&f.name))); }
        BlockKind::Raw { xml, .. } => { let (text, link) = docxml::raw_summary(xml); if let Some(link) = link { out.push_str(&format!("<p><a href=\"{}\">{}</a></p>", esc(&link), esc(if text.is_empty() { &link } else { &text }))); } else { out.push_str(&format!("<pre>{}</pre>", esc(&text))); } }
    }
}

fn rich_html(text: &RichText) -> String {
    let mut out = String::new();
    for (range, marks) in text.spans() {
        let mut s = match marks.inline.as_ref().map(|i| &i.kind) {
            Some(InlineKind::Equation(src)) => format!("\\({}\\)", esc(src)),
            Some(InlineKind::Button(b)) => esc(&b.text),
            Some(InlineKind::Reminder(r)) => esc(&r.label(" ")),
            Some(InlineKind::DocLink(l)) => esc(&l.title),
            Some(InlineKind::File(f)) => esc(&f.name),
            Some(InlineKind::Other { .. }) => "￼".into(),
            None => esc(&text.text[range]),
        };
        if let Some(link) = &marks.link { s = format!("<a href=\"{}\">{s}</a>", esc(link)); }
        if marks.code { s = format!("<code>{s}</code>"); }
        if marks.bold { s = format!("<strong>{s}</strong>"); }
        if marks.italic { s = format!("<em>{s}</em>"); }
        if marks.underline { s = format!("<u>{s}</u>"); }
        if marks.strike { s = format!("<del>{s}</del>"); }
        out.push_str(&s);
    }
    out
}

fn data_uri(root: &Path, src: &str) -> Option<String> {
    let path = asset_path(root, src)?;
    let bytes = fs::read(&path).ok()?;
    let mime = match path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() { "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "gif" => "image/gif", "svg" => "image/svg+xml", "webp" => "image/webp", _ => "application/octet-stream" };
    Some(format!("data:{mime};base64,{}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn emit_success(data: Value, options: &Options) {
    let mut envelope = json!({ "ok": true, "identity": "local", "data": data, "meta": { "library": library_root(options) } });
    if let Some(expr) = &options.jq { envelope = apply_jq(envelope, expr); }
    print_value(&envelope, options.format);
}

fn emit_error(error: &CliError) {
    let value = json!({ "ok": false, "identity": "local", "error": { "type": error.kind, "message": error.message, "hint": error.hint } });
    eprintln!("{}", serde_json::to_string(&value).unwrap_or_else(|_| "{\"ok\":false}".into()));
}

fn print_value(value: &Value, format: OutputFormat) {
    match format {
        OutputFormat::Json | OutputFormat::Ndjson => println!("{}", serde_json::to_string(value).unwrap_or_else(|_| "null".into())),
        OutputFormat::Pretty => println!("{}", serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into())),
        OutputFormat::Table => println!("{}", table_text(value)),
        OutputFormat::Csv => println!("{}", csv_text(value)),
    }
}

fn table_text(value: &Value) -> String {
    let rows = rows_value(value);
    if rows.is_empty() { return serde_json::to_string_pretty(value).unwrap_or_default(); }
    let mut keys = BTreeMap::<String, usize>::new();
    for row in &rows { if let Some(obj) = row.as_object() { for k in obj.keys() { *keys.entry(k.clone()).or_default() += 1; } } }
    let headers: Vec<String> = keys.keys().cloned().collect();
    let mut out = vec![headers.join("\t")];
    for row in rows { out.push(headers.iter().map(|k| value_cell(row.get(k).unwrap_or(&Value::Null))).collect::<Vec<_>>().join("\t")); }
    out.join("\n")
}

fn csv_text(value: &Value) -> String {
    let rows = rows_value(value);
    if rows.is_empty() { return String::new(); }
    let mut keys = BTreeMap::<String, usize>::new();
    for row in &rows { if let Some(obj) = row.as_object() { for k in obj.keys() { *keys.entry(k.clone()).or_default() += 1; } } }
    let headers: Vec<String> = keys.keys().cloned().collect();
    let mut out = vec![headers.iter().map(|h| csv_cell(h)).collect::<Vec<_>>().join(",")];
    for row in rows { out.push(headers.iter().map(|k| csv_cell(&value_cell(row.get(k).unwrap_or(&Value::Null)))).collect::<Vec<_>>().join(",")); }
    out.join("\n")
}

fn rows_value(value: &Value) -> Vec<Value> {
    value.get("data").and_then(|d| d.get("items")).and_then(Value::as_array).cloned().or_else(|| value.get("data").and_then(Value::as_array).cloned()).unwrap_or_default()
}
fn value_cell(v: &Value) -> String { match v { Value::Null => String::new(), Value::String(s) => s.clone(), _ => v.to_string() } }
fn csv_cell(s: &str) -> String { if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.into() } }

fn apply_jq(value: Value, expr: &str) -> Value {
    let mut current = value;
    let expr = expr.trim().strip_prefix('.').unwrap_or(expr.trim());
    for part in expr.split('.') {
        if part.is_empty() { continue; }
        if let Some(name) = part.strip_suffix("[]") {
            current = current.get(name).and_then(Value::as_array).map(|a| Value::Array(a.clone())).unwrap_or(Value::Null);
        } else {
            current = current.get(part).cloned().unwrap_or(Value::Null);
        }
    }
    current
}

fn print_help(args: &[String]) {
    let topic = args.first().map(String::as_str);
    match topic {
        Some("docs") | Some("doc") => println!(r#"quiet docs — local Quiet.md document operations

AGENT QUICKSTART
  quiet docs +list                                      # list documents
  quiet docs +get <id>                                  # read metadata and content
  quiet docs +search <query>                            # search title and body
  quiet docs +create --title TITLE --content TEXT       # create Markdown document
  quiet docs +export <id> --as markdown --output x.md   # export

SHORTCUTS
  +list +get +create +update +append +rename +duplicate +move +delete +metadata
  +search +import +export +pin +star +width +reading +catalogue +info +refs
  +links +stats +print

Writes support --dry-run. Delete and print require --yes. Use --content-format xml
for complete DocxXML access to equations, buttons, reminders, countdowns, files,
images, covers, columns, merged tables and unknown extensions.

Global flags: --library PATH --format json|pretty|table|ndjson|csv --jq EXPR --dry-run --yes
"#),
        Some("library") => println!(r#"quiet library — inspect the local library

COMMANDS
  path | status | list | tree | doctor | validate | settings get|set
"#),
        Some("trash") => println!(r#"quiet trash — manage deleted documents

COMMANDS
  list
  restore <id> --yes
  purge <id> --yes
  empty --yes
"#),
        Some("assets") => println!(r#"quiet assets — list local image and attachment references

USAGE
  quiet assets [DOCUMENT_ID]
"#),
        Some("emoji") => println!(r#"quiet emoji — search the same embedded emoji data as Quiet.md

USAGE
  quiet emoji search QUERY
"#),
        Some("schema") => println!(r#"quiet schema [COMMAND]

Inspect command risk, description and usage. The schema uses the same Agent-first
shape as lark-cli but describes local operations only.
"#),
        _ => println!(r#"quiet — Quiet.md local-first CLI

AGENT QUICKSTART
  Browse commands:  quiet <domain> --help
  Inspect a call:   quiet schema <domain> <command>
  Prefer a +shortcut over a lower-level command when one matches the task.
  Risk: each write command documents read | write | high-risk-write.
  On writes: --dry-run previews; high-risk writes require --yes.

USAGE
  quiet <command> [subcommand] [flags]

DOMAINS
  docs       Create, read, update, search, move, export and delete local documents
  library    Path, status, tree, settings and integrity checks
  trash      List, restore, purge and empty deleted documents
  assets     Inspect local image and attachment references
  emoji      Search embedded emoji data
  schema     Inspect command parameters and risk
  app        Open the Quiet.md GUI

OUTPUT
  --format json|pretty|table|ndjson|csv   JSON is the default
  --jq <expr>                             Simple safe field selection
  --library <path>                        Use a separate library

Quiet is local. It does not implement Feishu OAuth, messaging, calendar, mail,
Base, tasks, approvals or other cloud services; use lark-cli for those domains.
"#),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_global_format_and_library() {
        let (o, args) = parse_global(vec!["--format".into(), "pretty".into(), "--library=/tmp/x".into(), "docs".into(), "+list".into()]).unwrap();
        assert_eq!(o.format, OutputFormat::Pretty);
        assert_eq!(o.library, Some(PathBuf::from("/tmp/x")));
        assert_eq!(args, vec!["docs", "+list"]);
    }

    #[test]
    fn applies_safe_jq_path() {
        let v = apply_jq(json!({"data":{"id":"x"}}), ".data.id");
        assert_eq!(v, json!("x"));
    }

    #[test]
    fn html_escapes_content() {
        let doc = Document { title: RichText::plain("<title>"), blocks: vec![Block::text("<&")], ..Default::default() };
        let html = render_html(&doc, &[], Path::new("/tmp"));
        assert!(html.contains("&lt;title&gt;"));
        assert!(html.contains("&lt;&amp;"));
    }
}
