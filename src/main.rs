use serde::Deserialize;
use serde::de::value::Error;
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::process;
use syn::visit::Visit;
use tower_lsp::jsonrpc::Result as TowerResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

const RAW_JSON_URL: &str = "https://raw.githubusercontent.com/cryspen/rust-core-models/refs/heads/main/tools/core-coverage/coverage.json";
const COVERAGE_FILENAME: &str = "coverage.json";

#[derive(Debug, Deserialize)]
pub struct Coverage {
    pub core: Core,
}

#[derive(Debug, Deserialize)]
pub struct Core {
    pub modules: Vec<Module>,
}

#[derive(Debug, Deserialize)]
pub struct Module {
    pub module: String,
    pub in_scope: bool,
    pub missing_items: Vec<String>,
    pub supported_items: Option<Vec<String>>,
}

/// Safely resolves a local file path inside a target workspace root
fn get_coverage_json() -> io::Result<PathBuf> {
    let exe = env::current_exe()?;

    let extension_dir = exe
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "can't find executable directory"))?;

    Ok(extension_dir.join(COVERAGE_FILENAME))
}

fn download_json_file(destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Downloading coverage.json...");

    let response = ureq::get(RAW_JSON_URL).call()?;
    let mut reader = response.into_reader();

    let mut file = File::create(destination)?;
    io::copy(&mut reader, &mut file)?;

    eprintln!("Downloaded to {:?}", destination);
    Ok(())
}

fn setup_list() -> Result<PathBuf, Box<dyn std::error::Error>> {
    // 1. Enforce strict structural isolation rules via path verification
    let json_config_path = match get_coverage_json() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("coverage.json not in workspace root: {}", err);
            process::exit(1);
        }
    };

    // 2. Fetch and synchronize missing metadata files on-demand
    if !json_config_path.exists() {
        if let Err(e) = download_json_file(&json_config_path) {
            eprintln!("Failed to fetch coverage.json: {}", e);
            process::exit(1);
        }
    }
    Ok(json_config_path)
}

pub fn process_json(json_path: PathBuf) -> Result<Coverage, Error> {
    // 3. Process json
    let json_file = File::open(&json_path).expect("failed to open coverage.json");
    let reader = BufReader::new(json_file);

    let coverage: Coverage = serde_json::from_reader(reader)
        .expect("Mismatched metadata properties inside hax-coverage.json file");
    Ok(coverage)
}

// --- 1. LINTING CORE LOGIC ---
use std::collections::HashMap;

struct LintVisitor<'a> {
    coverage: &'a Coverage,
    unsupported_methods: &'a HashSet<String>,
    imports: HashMap<String, String>,
    local_types: HashMap<String, String>,
    diagnostics: Vec<Diagnostic>,
}

impl<'ast> Visit<'ast> for LintVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        for input in &node.sig.inputs {
            let syn::FnArg::Typed(arg) = input else {
                continue;
            };

            let syn::Pat::Ident(id) = &*arg.pat else {
                continue;
            };

            let syn::Type::Path(ty) = &*arg.ty else {
                continue;
            };

            let ty = ty
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");

            self.local_types.insert(id.ident.to_string(), ty);
        }

        syn::visit::visit_item_fn(self, node);
    }

    fn visit_local(&mut self, node: &'ast syn::Local) {
        let syn::Pat::Type(pat_type) = &node.pat else {
            syn::visit::visit_local(self, node);
            return;
        };

        let syn::Pat::Ident(ident) = &*pat_type.pat else {
            syn::visit::visit_local(self, node);
            return;
        };

        let syn::Type::Path(ty) = &*pat_type.ty else {
            syn::visit::visit_local(self, node);
            return;
        };

        let ty = ty
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");

        self.local_types.insert(ident.ident.to_string(), ty);

        syn::visit::visit_local(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        if let syn::Expr::Path(receiver) = &*node.receiver {
            if let Some(id) = receiver.path.get_ident() {
                if let Some(ty) = self.local_types.get(&id.to_string()) {
                    let ty = self.imports.get(ty).cloned().unwrap_or_else(|| ty.clone());

                    let candidate = format!("{}::{}", ty, node.method);

                    if self.unsupported_methods.contains(&candidate) {
                        let start = node.method.span().start();
                        let end = node.method.span().end();

                        self.diagnostics.push(Diagnostic {
                            range: Range::new(
                                Position::new((start.line - 1) as u32, start.column as u32),
                                Position::new((end.line - 1) as u32, end.column as u32),
                            ),
                            severity: Some(DiagnosticSeverity::WARNING),
                            message: format!("Use of unsupported core model: `{}`", candidate),
                            source: Some("haxlint".into()),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        syn::visit::visit_expr_method_call(self, node);
    }
    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        fn collect(
            tree: &syn::UseTree,
            prefix: String,
            imports: &mut HashMap<String, String>,
            unsupported: &HashSet<String>,
            diagnostics: &mut Vec<Diagnostic>,
        ) {
            match tree {
                syn::UseTree::Path(path) => {
                    let prefix = if prefix.is_empty() {
                        path.ident.to_string()
                    } else {
                        format!("{}::{}", prefix, path.ident)
                    };

                    collect(&*path.tree, prefix, imports, unsupported, diagnostics);
                }

                syn::UseTree::Name(name) => {
                    let full = if prefix.is_empty() {
                        name.ident.to_string()
                    } else {
                        format!("{}::{}", prefix, name.ident)
                    };

                    // try without
                    let normalized = full
                        .strip_prefix("std::")
                        .or_else(|| full.strip_prefix("core::"))
                        .unwrap_or(&full);

                    // prev: if unsupported.contains(&full)
                    if unsupported.contains(normalized) {
                        let start = name.ident.span().start();
                        let end = name.ident.span().end();

                        diagnostics.push(Diagnostic {
                            range: Range::new(
                                Position::new((start.line - 1) as u32, start.column as u32),
                                Position::new((end.line - 1) as u32, end.column as u32),
                            ),
                            severity: Some(DiagnosticSeverity::WARNING),
                            message: format!("Import of unsupported core model: `{}`", full),
                            source: Some("haxlint".to_string()),
                            ..Default::default()
                        });
                    }

                    imports.insert(name.ident.to_string(), full);
                }

                syn::UseTree::Rename(rename) => {
                    let full = if prefix.is_empty() {
                        rename.ident.to_string()
                    } else {
                        format!("{}::{}", prefix, rename.ident)
                    };

                    // start change
                    if unsupported.contains(&full) {
                        let start = rename.rename.span().start();
                        let end = rename.rename.span().end();

                        diagnostics.push(Diagnostic {
                            range: Range::new(
                                Position::new((start.line - 1) as u32, start.column as u32),
                                Position::new((end.line - 1) as u32, end.column as u32),
                            ),
                            severity: Some(DiagnosticSeverity::WARNING),
                            message: format!("Import of unsupported core model: `{}`", full),
                            source: Some("haxlint".to_string()),
                            ..Default::default()
                        });
                    }
                    // end change

                    imports.insert(rename.rename.to_string(), full);
                }

                syn::UseTree::Group(group) => {
                    for item in &group.items {
                        collect(item, prefix.clone(), imports, unsupported, diagnostics);
                    }
                }

                _ => {}
            }
        }

        collect(
            &node.tree,
            String::new(),
            &mut self.imports,
            self.unsupported_methods,
            &mut self.diagnostics,
        );

        syn::visit::visit_item_use(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(expr_path) = &*node.func {
            let segments = &expr_path.path.segments;

            if segments.len() >= 2 {
                let qualified = segments
                    .iter()
                    .take(segments.len() - 1)
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::");

                if self.unsupported_methods.contains(&qualified) {
                    let ident = &segments[segments.len() - 2].ident;

                    let start = ident.span().start();
                    let end = ident.span().end();

                    self.diagnostics.push(Diagnostic {
                        range: Range::new(
                            Position::new((start.line - 1) as u32, start.column as u32),
                            Position::new((end.line - 1) as u32, end.column as u32),
                        ),
                        severity: Some(DiagnosticSeverity::WARNING),
                        message: format!("Use of unsupported core model: `{}`", qualified),
                        source: Some("haxlint".to_string()),
                        ..Default::default()
                    });
                }
            }
        }

        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        let qualified = node
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");

        // Exact match.
        if self.unsupported_methods.contains(&qualified) {
            if let Some(last) = node.path.segments.last() {
                let start = last.ident.span().start();
                let end = last.ident.span().end();

                self.diagnostics.push(Diagnostic {
                    range: Range::new(
                        Position::new((start.line - 1) as u32, start.column as u32),
                        Position::new((end.line - 1) as u32, end.column as u32),
                    ),
                    severity: Some(DiagnosticSeverity::WARNING),
                    message: format!("Use of unsupported core model: `{}`", qualified),
                    source: Some("haxlint".into()),
                    ..Default::default()
                });
            }
        }
        // Fallback: module is known to have partial support.
        else if let Some(first) = node.path.segments.first() {
            let module = first.ident.to_string();

            if let Some(info) = self
                .coverage
                .core
                .modules
                .iter()
                .find(|m| m.in_scope && m.module == module && !m.missing_items.is_empty())
            {
                let start = first.ident.span().start();
                let end = first.ident.span().end();

                self.diagnostics.push(Diagnostic {
                    range: Range::new(
                        Position::new((start.line - 1) as u32, start.column as u32),
                        Position::new((end.line - 1) as u32, end.column as u32),
                    ),
                    severity: Some(DiagnosticSeverity::INFORMATION),
                    message: format!(
                        "Module `{}` has partial support.\nUnsupported items:\n{}",
                        module,
                        info.missing_items.join("\n")
                    ),
                    source: Some("haxlint".into()),
                    ..Default::default()
                });
            }
        }

        syn::visit::visit_expr_path(self, node);
    }
} // --- 2. LANGUAGE SERVER STATE & IMPLEMENTATION ---

struct Backend {
    client: Client,
    coverage: Coverage,
    unsupported_methods: HashSet<String>,
}

impl Backend {
    fn run_lint(&self, text: &str) -> Vec<Diagnostic> {
        let mut visitor = LintVisitor {
            coverage: &self.coverage,
            unsupported_methods: &self.unsupported_methods,
            imports: HashMap::new(),
            local_types: HashMap::new(),
            diagnostics: Vec::new(),
        };

        if let Ok(syntax_tree) = syn::parse_file(text) {
            visitor.visit_file(&syntax_tree);
        }
        visitor.diagnostics
    }

    async fn validate_document(&self, uri: Url, text: String) {
        let diagnostics = self.run_lint(&text);
        let num_entries = &self.unsupported_methods.len();
        // Instantly push error indicators directly to the editor workspace
        self.client
            .log_message(
                MessageType::INFO,
                format!("Ran validate_document ({} entries)", num_entries),
            )
            .await;
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> TowerResult<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // Tells VS Code to send full text documents on open/save/change
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn shutdown(&self) -> TowerResult<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.validate_document(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, mut params: DidChangeTextDocumentParams) {
        // Grab the latest textual modifications from the editor buffer
        if let Some(change) = params.content_changes.pop() {
            self.validate_document(params.text_document.uri, change.text)
                .await;
        }
    }
}

// --- 3. BINARY ENTRYPOINT ---

#[tokio::main]
async fn main() {
    let coverage = setup_list().expect("failed to set up coverage.json");
    let coverage = process_json(coverage).expect("failed to parse coverage.json");

    let disallowed: HashSet<String> = coverage
        .core
        .modules
        .iter()
        .flat_map(|m| {
            m.missing_items
                .iter()
                .map(move |item| format!("{}::{}", m.module, item))
        })
        .map(|s| {
            s.strip_prefix("std::")
                .or_else(|| s.strip_prefix("core::"))
                .unwrap_or(&s)
                .to_string()
        })
        .collect();

    eprintln!("Loaded {} items", disallowed.len());

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        coverage,
        unsupported_methods: disallowed,
    });

    // Run the messaging transport pipeline directly over standard local system I/O streams
    Server::new(stdin, stdout, socket).serve(service).await;
}
