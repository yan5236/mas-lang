use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{
    AssignTarget, ConditionalBranch, Expr, ImportItem, ImportKind, ImportPath, MapEntry, Program,
    Stmt,
};
use crate::diagnostic::CompilerError;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::settings::CompilerSettings;
use crate::source::{SourceFile, Span};

/// 保存模块装载后的扁平化源码与最终程序，供后续语义与代码生成复用。
pub struct ModuleLoadResult {
    pub source: SourceFile,
    pub program: Program,
    pub package_roots: Vec<PathBuf>,
}

/// 从入口源码开始装载整个模块图，并把模块重写为单个可编译程序。
pub fn load_module_graph(
    entry: &SourceFile,
    settings: &CompilerSettings,
) -> Result<ModuleLoadResult, CompilerError> {
    let Some(entry_path) = canonicalize_existing_path(Path::new(entry.path())) else {
        return parse_single_source(entry, settings);
    };
    let Some(project_root) = discover_project_root(&entry_path) else {
        return parse_single_source(entry, settings);
    };
    let entry_package_root = discover_package_root(&entry_path, &project_root)?;
    let mut loader = ModuleLoader::new(settings, project_root);
    loader.load_module(&entry_path, &entry_package_root, true)?;
    Ok(ModuleLoadResult {
        source: SourceFile::new(entry.path(), loader.synthetic_source),
        program: Program {
            statements: loader.statements,
        },
        package_roots: loader.package_roots.into_iter().collect(),
    })
}

/// 维护模块装载过程中的缓存、循环检测和最终扁平化结果。
struct ModuleLoader<'a> {
    settings: &'a CompilerSettings,
    project_root: PathBuf,
    loaded_modules: BTreeMap<PathBuf, LoadedModule>,
    active_modules: HashSet<PathBuf>,
    dependency_cache: BTreeMap<PathBuf, BTreeSet<String>>,
    package_roots: BTreeSet<PathBuf>,
    synthetic_source: String,
    statements: Vec<Stmt>,
}

/// 保存单个模块对外可见的公开符号映射。
#[derive(Clone)]
struct LoadedModule {
    exports: BTreeMap<String, String>,
}

/// 基于模块作用域重写标识符，把导入名和顶层声明名都替换成内部稳定名。
struct ModuleRewriter {
    bindings: BTreeMap<String, String>,
    local_scopes: Vec<HashSet<String>>,
}

impl<'a> ModuleLoader<'a> {
    /// 为一次模块装载流程创建新的装载器实例。
    fn new(settings: &'a CompilerSettings, project_root: PathBuf) -> Self {
        Self {
            settings,
            project_root,
            loaded_modules: BTreeMap::new(),
            active_modules: HashSet::new(),
            dependency_cache: BTreeMap::new(),
            package_roots: BTreeSet::new(),
            synthetic_source: String::new(),
            statements: Vec::new(),
        }
    }

    /// 装载单个模块文件，递归处理其导入关系并缓存公开符号表。
    fn load_module(
        &mut self,
        module_path: &Path,
        package_root: &Path,
        is_entry: bool,
    ) -> Result<LoadedModule, CompilerError> {
        if let Some(loaded) = self.loaded_modules.get(module_path) {
            return Ok(loaded.clone());
        }

        self.package_roots.insert(package_root.to_path_buf());

        if !self.active_modules.insert(module_path.to_path_buf()) {
            return Err(CompilerError::build_message(
                format!("模块图中检测到循环依赖：{}", module_path.display()),
                Span::new(0, 0),
            ));
        }

        let source = read_module_source(module_path)?;
        let tokens = Lexer::new(&source, self.settings).tokenize()?;
        let program = Parser::new(tokens, self.settings).parse_program()?;
        let renamed_declarations = collect_top_level_declarations(&program, module_path, is_entry);
        let mut imports = BTreeMap::new();
        let mut exports = BTreeMap::new();

        for statement in &program.statements {
            match statement {
                Stmt::Import { path, kind, .. } => {
                    self.resolve_import_bindings(package_root, path, kind, &mut imports)?
                }
                Stmt::ExportImport { path, kind, .. } => self.resolve_export_bindings(
                    package_root,
                    path,
                    kind,
                    &mut imports,
                    &mut exports,
                )?,
                Stmt::FunctionDecl {
                    is_public, name, ..
                }
                | Stmt::ExternFunctionDecl {
                    is_public, name, ..
                }
                | Stmt::VarDecl {
                    is_public, name, ..
                } => {
                    if *is_public {
                        let internal = renamed_declarations.get(name).ok_or_else(|| {
                            CompilerError::build_message(
                                format!("公开符号 `{name}` 缺少内部重写目标"),
                                statement.span(),
                            )
                        })?;
                        exports.insert(name.clone(), internal.clone());
                    }
                }
                _ => {}
            }
        }

        for name in imports.keys() {
            if renamed_declarations.contains_key(name) {
                return Err(CompilerError::build_message(
                    format!("导入名 `{name}` 与当前模块顶层声明重名"),
                    Span::new(0, 0),
                ));
            }
        }

        let mut bindings = renamed_declarations;
        bindings.extend(imports);
        let mut rewriter = ModuleRewriter::new(bindings);
        let rewritten_statements = rewriter.rewrite_program(program);
        let offset = append_module_source(&mut self.synthetic_source, module_path, &source);
        self.statements.extend(
            rewritten_statements
                .into_iter()
                .filter(|statement| {
                    !matches!(statement, Stmt::Import { .. } | Stmt::ExportImport { .. })
                })
                .map(|statement| shift_stmt(statement, offset)),
        );

        let loaded = LoadedModule { exports };
        self.loaded_modules
            .insert(module_path.to_path_buf(), loaded.clone());
        self.active_modules.remove(module_path);
        Ok(loaded)
    }

    /// 解析普通 `import` 语句导入的绑定名，并写入当前模块可见绑定表。
    fn resolve_import_bindings(
        &mut self,
        package_root: &Path,
        path: &ImportPath,
        kind: &ImportKind,
        imports: &mut BTreeMap<String, String>,
    ) -> Result<(), CompilerError> {
        let (target_file, target_package_root) = self.resolve_module_path(package_root, path)?;
        let target_module = self.load_module(&target_file, &target_package_root, false)?;
        merge_imports(kind, &target_module.exports, imports, path.span)?;
        Ok(())
    }

    /// 解析 `export import` 语句，并同时更新导入绑定和公开导出表。
    fn resolve_export_bindings(
        &mut self,
        package_root: &Path,
        path: &ImportPath,
        kind: &ImportKind,
        imports: &mut BTreeMap<String, String>,
        exports: &mut BTreeMap<String, String>,
    ) -> Result<(), CompilerError> {
        let (target_file, target_package_root) = self.resolve_module_path(package_root, path)?;
        let target_module = self.load_module(&target_file, &target_package_root, false)?;
        let merged = merge_imports(kind, &target_module.exports, imports, path.span)?;
        exports.extend(merged);
        Ok(())
    }

    /// 根据导入路径决定应解析为当前包本地模块，还是项目根目录中的已安装依赖模块。
    fn resolve_module_path(
        &mut self,
        package_root: &Path,
        path: &ImportPath,
    ) -> Result<(PathBuf, PathBuf), CompilerError> {
        let segments = path.normalized_segments();
        if segments.is_empty() {
            return Err(CompilerError::syntax_message("导入路径不能为空", path.span));
        }

        let dependency_names = self.read_dependency_names(package_root)?;
        if !path.is_local() && dependency_names.contains(&segments[0]) {
            let dependency_root = self.find_dependency_root(&segments[0])?;
            let target =
                resolve_module_file(&dependency_root.join("src"), &segments[1..], path.span)?;
            return Ok((target, dependency_root));
        }

        let target = resolve_module_file(&package_root.join("src"), segments, path.span)?;
        Ok((target, package_root.to_path_buf()))
    }

    /// 读取当前包 `mod.yml` 中声明的依赖名集合，供导入解析顺序判断复用。
    fn read_dependency_names(
        &mut self,
        package_root: &Path,
    ) -> Result<BTreeSet<String>, CompilerError> {
        if let Some(cached) = self.dependency_cache.get(package_root) {
            return Ok(cached.clone());
        }

        let manifest_path = package_root.join("mod.yml");
        let text = fs::read_to_string(&manifest_path).map_err(|error| {
            CompilerError::build_message(
                format!("读取依赖清单失败：{}: {error}", manifest_path.display()),
                Span::new(0, 0),
            )
        })?;
        let names = parse_dependency_names(&text);
        self.dependency_cache
            .insert(package_root.to_path_buf(), names.clone());
        Ok(names)
    }

    /// 在当前项目根目录的 `mod_module/` 平铺缓存中定位依赖包根目录。
    fn find_dependency_root(&self, dependency_name: &str) -> Result<PathBuf, CompilerError> {
        let store_root = self.project_root.join("mod_module");
        let mut matches = Vec::new();
        for entry in fs::read_dir(&store_root).map_err(|error| {
            CompilerError::build_message(
                format!("读取依赖缓存目录失败：{}: {error}", store_root.display()),
                Span::new(0, 0),
            )
        })? {
            let entry = entry.map_err(|error| {
                CompilerError::build_message(format!("读取依赖缓存失败：{error}"), Span::new(0, 0))
            })?;
            let path = entry.path();
            if path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&format!("{dependency_name}@")))
            {
                matches.push(path);
            }
        }

        match matches.len() {
            1 => Ok(matches.remove(0)),
            0 => Err(CompilerError::build_message(
                format!("未安装依赖 `{dependency_name}`，请先执行 fama install"),
                Span::new(0, 0),
            )),
            _ => Err(CompilerError::build_message(
                format!("依赖 `{dependency_name}` 出现多个缓存版本，请先解决冲突"),
                Span::new(0, 0),
            )),
        }
    }
}

impl ModuleRewriter {
    /// 使用当前模块可见绑定表创建新的符号重写器。
    fn new(bindings: BTreeMap<String, String>) -> Self {
        Self {
            bindings,
            local_scopes: vec![HashSet::new()],
        }
    }

    /// 重写整个模块程序，统一替换导入名和顶层声明名。
    fn rewrite_program(&mut self, program: Program) -> Vec<Stmt> {
        program
            .statements
            .into_iter()
            .map(|statement| self.rewrite_stmt(statement, true))
            .collect()
    }

    /// 按作用域重写单条语句中的声明名和引用名。
    fn rewrite_stmt(&mut self, statement: Stmt, is_top_level: bool) -> Stmt {
        match statement {
            Stmt::FunctionDecl {
                is_public: _,
                name,
                parameters,
                return_type,
                body,
                span,
            } => {
                let new_name = if is_top_level {
                    self.rename_name(&name)
                } else {
                    name.clone()
                };
                self.local_scopes
                    .push(parameters.iter().map(|item| item.name.clone()).collect());
                let body = body
                    .into_iter()
                    .map(|nested| self.rewrite_stmt(nested, false))
                    .collect();
                self.local_scopes.pop();
                Stmt::FunctionDecl {
                    is_public: false,
                    name: new_name,
                    parameters,
                    return_type,
                    body,
                    span,
                }
            }
            Stmt::ExternFunctionDecl {
                is_public: _,
                name,
                parameters,
                return_type,
                span,
            } => Stmt::ExternFunctionDecl {
                is_public: false,
                name: if is_top_level {
                    self.rename_name(&name)
                } else {
                    name
                },
                parameters,
                return_type,
                span,
            },
            Stmt::VarDecl {
                is_public: _,
                binding_kind,
                name,
                annotation,
                initializer,
                span,
            } => {
                let initializer = initializer.map(|expr| self.rewrite_expr(expr));
                if !is_top_level {
                    self.current_scope_mut().insert(name.clone());
                }
                Stmt::VarDecl {
                    is_public: false,
                    binding_kind,
                    name: if is_top_level {
                        self.rename_name(&name)
                    } else {
                        name
                    },
                    annotation,
                    initializer,
                    span,
                }
            }
            Stmt::Assign {
                target,
                op,
                value,
                span,
            } => Stmt::Assign {
                target: self.rewrite_target(target),
                op,
                value: self.rewrite_expr(value),
                span,
            },
            Stmt::If {
                branches,
                else_branch,
                span,
            } => Stmt::If {
                branches: branches
                    .into_iter()
                    .map(|branch| ConditionalBranch {
                        condition: self.rewrite_expr(branch.condition),
                        body: self.rewrite_block(branch.body),
                        span: branch.span,
                    })
                    .collect(),
                else_branch: else_branch.map(|body| self.rewrite_block(body)),
                span,
            },
            Stmt::While {
                condition,
                body,
                span,
            } => Stmt::While {
                condition: self.rewrite_expr(condition),
                body: self.rewrite_block(body),
                span,
            },
            Stmt::ForRange {
                name,
                start,
                end,
                body,
                span,
            } => {
                self.local_scopes.push(HashSet::from([name.clone()]));
                let body = body
                    .into_iter()
                    .map(|item| self.rewrite_stmt(item, false))
                    .collect();
                self.local_scopes.pop();
                Stmt::ForRange {
                    name,
                    start: self.rewrite_expr(start),
                    end: self.rewrite_expr(end),
                    body,
                    span,
                }
            }
            Stmt::ForEach {
                key_name,
                value_name,
                iterable,
                body,
                span,
            } => {
                let mut scope = HashSet::from([key_name.clone()]);
                if let Some(value_name) = &value_name {
                    scope.insert(value_name.clone());
                }
                self.local_scopes.push(scope);
                let body = body
                    .into_iter()
                    .map(|item| self.rewrite_stmt(item, false))
                    .collect();
                self.local_scopes.pop();
                Stmt::ForEach {
                    key_name,
                    value_name,
                    iterable: self.rewrite_expr(iterable),
                    body,
                    span,
                }
            }
            Stmt::Return { value, span } => Stmt::Return {
                value: value.map(|expr| self.rewrite_expr(expr)),
                span,
            },
            Stmt::ExprStmt { expr, span } => Stmt::ExprStmt {
                expr: self.rewrite_expr(expr),
                span,
            },
            other => other,
        }
    }

    /// 为嵌套代码块创建独立的局部作用域，避免错误替换局部变量名。
    fn rewrite_block(&mut self, body: Vec<Stmt>) -> Vec<Stmt> {
        self.local_scopes.push(HashSet::new());
        let rewritten = body
            .into_iter()
            .map(|item| self.rewrite_stmt(item, false))
            .collect();
        self.local_scopes.pop();
        rewritten
    }

    /// 按照当前作用域规则重写赋值目标。
    fn rewrite_target(&mut self, target: AssignTarget) -> AssignTarget {
        match target {
            AssignTarget::Identifier { name, span } => AssignTarget::Identifier {
                name: self.rewrite_identifier(name),
                span,
            },
            AssignTarget::Index {
                target,
                index,
                span,
            } => AssignTarget::Index {
                target: Box::new(self.rewrite_expr(*target)),
                index: Box::new(self.rewrite_expr(*index)),
                span,
            },
        }
    }

    /// 递归重写表达式树中的标识符引用。
    fn rewrite_expr(&mut self, expr: Expr) -> Expr {
        match expr {
            Expr::Identifier { name, span } => Expr::Identifier {
                name: self.rewrite_identifier(name),
                span,
            },
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => Expr::Binary {
                left: Box::new(self.rewrite_expr(*left)),
                op,
                right: Box::new(self.rewrite_expr(*right)),
                span,
            },
            Expr::Unary { op, operand, span } => Expr::Unary {
                op,
                operand: Box::new(self.rewrite_expr(*operand)),
                span,
            },
            Expr::Call {
                callee,
                arguments,
                span,
            } => Expr::Call {
                callee: Box::new(self.rewrite_expr(*callee)),
                arguments: arguments
                    .into_iter()
                    .map(|item| self.rewrite_expr(item))
                    .collect(),
                span,
            },
            Expr::ListLiteral { elements, span } => Expr::ListLiteral {
                elements: elements
                    .into_iter()
                    .map(|item| self.rewrite_expr(item))
                    .collect(),
                span,
            },
            Expr::MapLiteral { entries, span } => Expr::MapLiteral {
                entries: entries
                    .into_iter()
                    .map(|entry| MapEntry {
                        key: self.rewrite_expr(entry.key),
                        value: self.rewrite_expr(entry.value),
                        span: entry.span,
                    })
                    .collect(),
                span,
            },
            Expr::ArrayInitializer {
                element_type,
                size,
                elements,
                span,
            } => Expr::ArrayInitializer {
                element_type,
                size,
                elements: elements
                    .into_iter()
                    .map(|item| self.rewrite_expr(item))
                    .collect(),
                span,
            },
            Expr::Index {
                target,
                index,
                span,
            } => Expr::Index {
                target: Box::new(self.rewrite_expr(*target)),
                index: Box::new(self.rewrite_expr(*index)),
                span,
            },
            Expr::Slice {
                target,
                start,
                end,
                span,
            } => Expr::Slice {
                target: Box::new(self.rewrite_expr(*target)),
                start: start.map(|expr| Box::new(self.rewrite_expr(*expr))),
                end: end.map(|expr| Box::new(self.rewrite_expr(*expr))),
                span,
            },
            Expr::MethodCall {
                target,
                method,
                arguments,
                span,
            } => Expr::MethodCall {
                target: Box::new(self.rewrite_expr(*target)),
                method,
                arguments: arguments
                    .into_iter()
                    .map(|item| self.rewrite_expr(item))
                    .collect(),
                span,
            },
            Expr::PropertyAccess {
                target,
                property,
                span,
            } => Expr::PropertyAccess {
                target: Box::new(self.rewrite_expr(*target)),
                property,
                span,
            },
            Expr::Grouped { expression, span } => Expr::Grouped {
                expression: Box::new(self.rewrite_expr(*expression)),
                span,
            },
            other => other,
        }
    }

    /// 判断当前标识符是否应当映射到内部名，而不是保持原始文本。
    fn rewrite_identifier(&self, name: String) -> String {
        if self.is_local_name(&name) {
            name
        } else {
            self.rename_name(&name)
        }
    }

    /// 返回当前名称在模块内部对应的稳定重写结果，不存在时保留原名。
    fn rename_name(&self, name: &str) -> String {
        self.bindings
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// 判断给定名称是否已在当前局部作用域中声明，从而不应被重写。
    fn is_local_name(&self, name: &str) -> bool {
        self.local_scopes
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    /// 返回当前可写局部作用域，用于登记新声明的局部变量名。
    fn current_scope_mut(&mut self) -> &mut HashSet<String> {
        self.local_scopes.last_mut().expect("局部作用域栈不能为空")
    }
}

fn collect_top_level_declarations(
    program: &Program,
    module_path: &Path,
    is_entry: bool,
) -> BTreeMap<String, String> {
    program
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Stmt::FunctionDecl { name, .. } | Stmt::VarDecl { name, .. } => Some((
                name.clone(),
                if is_entry && name == "main" {
                    "main".to_string()
                } else {
                    format!("__ms_{}_{}", sanitize_module_name(module_path), name)
                },
            )),
            Stmt::ExternFunctionDecl { name, .. } => Some((name.clone(), name.clone())),
            _ => None,
        })
        .collect()
}

fn merge_imports(
    kind: &ImportKind,
    exports: &BTreeMap<String, String>,
    imports: &mut BTreeMap<String, String>,
    span: Span,
) -> Result<BTreeMap<String, String>, CompilerError> {
    let mut inserted = BTreeMap::new();
    match kind {
        ImportKind::All => {
            for (name, internal) in exports {
                imports.insert(name.clone(), internal.clone());
                inserted.insert(name.clone(), internal.clone());
            }
        }
        ImportKind::Named(items) => {
            for ImportItem { name, alias, .. } in items {
                let internal = exports.get(name).ok_or_else(|| {
                    CompilerError::build_message(
                        format!("导入的符号 `{name}` 不存在或未公开"),
                        span,
                    )
                })?;
                let visible_name = alias.clone().unwrap_or_else(|| name.clone());
                imports.insert(visible_name.clone(), internal.clone());
                inserted.insert(visible_name, internal.clone());
            }
        }
    }
    Ok(inserted)
}

fn parse_dependency_names(text: &str) -> BTreeSet<String> {
    let mut dependencies = BTreeSet::new();
    let mut in_dependencies = false;
    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim_start();
        if trimmed == "dependencies:" || trimmed == "dependencies: {}" {
            in_dependencies = true;
            continue;
        }
        if !raw_line.starts_with("  ") {
            in_dependencies = false;
        }
        if in_dependencies
            && raw_line.starts_with("  ")
            && !raw_line.starts_with("    ")
            && trimmed.ends_with(':')
        {
            dependencies.insert(trimmed.trim_end_matches(':').to_string());
        }
    }
    dependencies
}

fn resolve_module_file(
    module_root: &Path,
    segments: &[String],
    span: Span,
) -> Result<PathBuf, CompilerError> {
    let mut path = module_root.to_path_buf();
    for segment in segments {
        path.push(segment);
    }
    let direct_file = path.with_extension("ms");
    if direct_file.is_file() {
        return canonicalize_path(&direct_file);
    }
    let index_file = path.join("index.ms");
    if index_file.is_file() {
        return canonicalize_path(&index_file);
    }
    Err(CompilerError::build_message(
        format!("找不到模块文件：{}", path.display()),
        span,
    ))
}

fn append_module_source(output: &mut String, module_path: &Path, source: &SourceFile) -> usize {
    let header = format!("// module: {}\n", module_path.display());
    output.push_str(&header);
    let offset = output.len();
    output.push_str(source.text());
    if !source.text().ends_with('\n') {
        output.push('\n');
    }
    output.push('\n');
    offset
}

fn read_module_source(path: &Path) -> Result<SourceFile, CompilerError> {
    let text = fs::read_to_string(path).map_err(|error| {
        CompilerError::build_message(
            format!("读取模块失败：{}: {error}", path.display()),
            Span::new(0, 0),
        )
    })?;
    Ok(SourceFile::new(path.display().to_string(), text))
}

fn discover_project_root(entry_path: &Path) -> Option<PathBuf> {
    let mut current = entry_path.parent();
    while let Some(path) = current {
        if path.join("mod.yml").is_file() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

fn discover_package_root(
    module_path: &Path,
    project_root: &Path,
) -> Result<PathBuf, CompilerError> {
    let mut current = module_path.parent();
    while let Some(path) = current {
        if path.join("mod.yml").is_file() && path.join("src").is_dir() {
            return Ok(path.to_path_buf());
        }
        if path == project_root {
            break;
        }
        current = path.parent();
    }
    Err(CompilerError::build_message(
        format!("无法定位模块所属包根目录：{}", module_path.display()),
        Span::new(0, 0),
    ))
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, CompilerError> {
    fs::canonicalize(path).map_err(|error| {
        CompilerError::build_message(
            format!("解析路径失败：{}: {error}", path.display()),
            Span::new(0, 0),
        )
    })
}

fn canonicalize_existing_path(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    fs::canonicalize(path).ok()
}

fn parse_single_source(
    source: &SourceFile,
    settings: &CompilerSettings,
) -> Result<ModuleLoadResult, CompilerError> {
    let tokens = Lexer::new(source, settings).tokenize()?;
    let program = Parser::new(tokens, settings).parse_program()?;
    Ok(ModuleLoadResult {
        source: source.clone(),
        program,
        package_roots: Vec::new(),
    })
}

fn sanitize_module_name(module_path: &Path) -> String {
    module_path
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn shift_stmt(statement: Stmt, offset: usize) -> Stmt {
    match statement {
        Stmt::FunctionDecl {
            is_public,
            name,
            parameters,
            return_type,
            body,
            span,
        } => Stmt::FunctionDecl {
            is_public,
            name,
            parameters: parameters
                .into_iter()
                .map(|mut item| {
                    item.span = shift_span(item.span, offset);
                    item
                })
                .collect(),
            return_type,
            body: body
                .into_iter()
                .map(|item| shift_stmt(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::ExternFunctionDecl {
            is_public,
            name,
            parameters,
            return_type,
            span,
        } => Stmt::ExternFunctionDecl {
            is_public,
            name,
            parameters: parameters
                .into_iter()
                .map(|mut item| {
                    item.span = shift_span(item.span, offset);
                    item
                })
                .collect(),
            return_type,
            span: shift_span(span, offset),
        },
        Stmt::VarDecl {
            is_public,
            binding_kind,
            name,
            annotation,
            initializer,
            span,
        } => Stmt::VarDecl {
            is_public,
            binding_kind,
            name,
            annotation,
            initializer: initializer.map(|expr| shift_expr(expr, offset)),
            span: shift_span(span, offset),
        },
        Stmt::Assign {
            target,
            op,
            value,
            span,
        } => Stmt::Assign {
            target: shift_target(target, offset),
            op,
            value: shift_expr(value, offset),
            span: shift_span(span, offset),
        },
        Stmt::If {
            branches,
            else_branch,
            span,
        } => Stmt::If {
            branches: branches
                .into_iter()
                .map(|branch| ConditionalBranch {
                    condition: shift_expr(branch.condition, offset),
                    body: branch
                        .body
                        .into_iter()
                        .map(|item| shift_stmt(item, offset))
                        .collect(),
                    span: shift_span(branch.span, offset),
                })
                .collect(),
            else_branch: else_branch.map(|body| {
                body.into_iter()
                    .map(|item| shift_stmt(item, offset))
                    .collect()
            }),
            span: shift_span(span, offset),
        },
        Stmt::While {
            condition,
            body,
            span,
        } => Stmt::While {
            condition: shift_expr(condition, offset),
            body: body
                .into_iter()
                .map(|item| shift_stmt(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::ForRange {
            name,
            start,
            end,
            body,
            span,
        } => Stmt::ForRange {
            name,
            start: shift_expr(start, offset),
            end: shift_expr(end, offset),
            body: body
                .into_iter()
                .map(|item| shift_stmt(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::ForEach {
            key_name,
            value_name,
            iterable,
            body,
            span,
        } => Stmt::ForEach {
            key_name,
            value_name,
            iterable: shift_expr(iterable, offset),
            body: body
                .into_iter()
                .map(|item| shift_stmt(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Stmt::Return { value, span } => Stmt::Return {
            value: value.map(|expr| shift_expr(expr, offset)),
            span: shift_span(span, offset),
        },
        Stmt::ExprStmt { expr, span } => Stmt::ExprStmt {
            expr: shift_expr(expr, offset),
            span: shift_span(span, offset),
        },
        other => other,
    }
}

fn shift_target(target: AssignTarget, offset: usize) -> AssignTarget {
    match target {
        AssignTarget::Identifier { name, span } => AssignTarget::Identifier {
            name,
            span: shift_span(span, offset),
        },
        AssignTarget::Index {
            target,
            index,
            span,
        } => AssignTarget::Index {
            target: Box::new(shift_expr(*target, offset)),
            index: Box::new(shift_expr(*index, offset)),
            span: shift_span(span, offset),
        },
    }
}

fn shift_expr(expr: Expr, offset: usize) -> Expr {
    match expr {
        Expr::Literal { value, span } => Expr::Literal {
            value,
            span: shift_span(span, offset),
        },
        Expr::Identifier { name, span } => Expr::Identifier {
            name,
            span: shift_span(span, offset),
        },
        Expr::Binary {
            left,
            op,
            right,
            span,
        } => Expr::Binary {
            left: Box::new(shift_expr(*left, offset)),
            op,
            right: Box::new(shift_expr(*right, offset)),
            span: shift_span(span, offset),
        },
        Expr::Unary { op, operand, span } => Expr::Unary {
            op,
            operand: Box::new(shift_expr(*operand, offset)),
            span: shift_span(span, offset),
        },
        Expr::Call {
            callee,
            arguments,
            span,
        } => Expr::Call {
            callee: Box::new(shift_expr(*callee, offset)),
            arguments: arguments
                .into_iter()
                .map(|item| shift_expr(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::ListLiteral { elements, span } => Expr::ListLiteral {
            elements: elements
                .into_iter()
                .map(|item| shift_expr(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::MapLiteral { entries, span } => Expr::MapLiteral {
            entries: entries
                .into_iter()
                .map(|entry| MapEntry {
                    key: shift_expr(entry.key, offset),
                    value: shift_expr(entry.value, offset),
                    span: shift_span(entry.span, offset),
                })
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::ArrayInitializer {
            element_type,
            size,
            elements,
            span,
        } => Expr::ArrayInitializer {
            element_type,
            size,
            elements: elements
                .into_iter()
                .map(|item| shift_expr(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::Index {
            target,
            index,
            span,
        } => Expr::Index {
            target: Box::new(shift_expr(*target, offset)),
            index: Box::new(shift_expr(*index, offset)),
            span: shift_span(span, offset),
        },
        Expr::Slice {
            target,
            start,
            end,
            span,
        } => Expr::Slice {
            target: Box::new(shift_expr(*target, offset)),
            start: start.map(|expr| Box::new(shift_expr(*expr, offset))),
            end: end.map(|expr| Box::new(shift_expr(*expr, offset))),
            span: shift_span(span, offset),
        },
        Expr::MethodCall {
            target,
            method,
            arguments,
            span,
        } => Expr::MethodCall {
            target: Box::new(shift_expr(*target, offset)),
            method,
            arguments: arguments
                .into_iter()
                .map(|item| shift_expr(item, offset))
                .collect(),
            span: shift_span(span, offset),
        },
        Expr::PropertyAccess {
            target,
            property,
            span,
        } => Expr::PropertyAccess {
            target: Box::new(shift_expr(*target, offset)),
            property,
            span: shift_span(span, offset),
        },
        Expr::Grouped { expression, span } => Expr::Grouped {
            expression: Box::new(shift_expr(*expression, offset)),
            span: shift_span(span, offset),
        },
    }
}

fn shift_span(span: Span, offset: usize) -> Span {
    Span::new(span.start + offset, span.end + offset)
}
